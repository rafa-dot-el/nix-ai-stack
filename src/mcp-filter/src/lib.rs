//! Which tools an MCP client sees, and under which names (ADR-0004, filter semantics).
//!
//! Pure: no I/O, no async. The proxy feeds in each server's tool list and the
//! configured rules; out comes a [`Catalog`] that names every exposed tool and maps
//! a call back to `(server, original tool)`.
//!
//! 1. Rules match the server's *original* tool name, so renames never break filters.
//! 2. Proxy side first: `include` (empty = everything), then `exclude` (exclude wins),
//!    then `rename`/`describe`, then the server's prefix.
//! 3. Client side (a profile): only the servers it lists; its `include`/`exclude`
//!    again match original names; its `rename` gives the final exposed name verbatim,
//!    its `describe` the final description.
//! 4. Two tools ending up with the same exposed name is an error, never resolved silently.
//! 5. A call by an unknown or hidden name, or by the original name of a renamed tool,
//!    resolves to nothing.

use std::collections::BTreeMap;

/// MCP tool annotations (spec `ToolAnnotations` hints); `None` means "not declared".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Annotations {
    pub read_only: Option<bool>,
    pub destructive: Option<bool>,
    pub idempotent: Option<bool>,
    pub open_world: Option<bool>,
}

/// One tool as a server lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub annotations: Annotations,
}

impl Tool {
    pub fn named(name: &str) -> Self {
        Tool { name: name.to_string(), ..Tool::default() }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FilterError {
    #[error("invalid glob {pattern:?}: {reason}")]
    InvalidGlob { pattern: String, reason: String },
    #[error("invalid regex {pattern:?}: {reason}")]
    InvalidRegex { pattern: String, reason: String },
    #[error("exposed name {name:?} is claimed by {first_server}:{first_tool} and {second_server}:{second_tool}")]
    Collision {
        name: String,
        first_server: String,
        first_tool: String,
        second_server: String,
        second_tool: String,
    },
}

/// One include/exclude entry: a glob, `re:<regex>`, or an annotations pattern.
#[derive(Clone, Debug)]
pub enum Matcher {
    Glob { pattern: String, matcher: globset::GlobMatcher },
    Regex(regex::Regex),
    /// Matches when every declared field equals the tool's declared value.
    Annotations(Annotations),
}

impl Matcher {
    /// `re:<regex>` is a regular expression (anchored to the whole name); anything else a glob.
    pub fn parse(text: &str) -> Result<Matcher, FilterError> {
        if let Some(expr) = text.strip_prefix("re:") {
            return regex::Regex::new(&format!("^(?:{expr})$"))
                .map(Matcher::Regex)
                .map_err(|e| FilterError::InvalidRegex { pattern: text.to_string(), reason: e.to_string() });
        }
        globset::GlobBuilder::new(text)
            .literal_separator(false)
            .build()
            .map(|g| Matcher::Glob { pattern: text.to_string(), matcher: g.compile_matcher() })
            .map_err(|e| FilterError::InvalidGlob { pattern: text.to_string(), reason: e.kind().to_string() })
    }

    pub fn annotations(pattern: Annotations) -> Matcher {
        Matcher::Annotations(pattern)
    }

    pub fn matches(&self, tool: &Tool) -> bool {
        match self {
            Matcher::Glob { matcher, .. } => matcher.is_match(&tool.name),
            Matcher::Regex(re) => re.is_match(&tool.name),
            Matcher::Annotations(want) => {
                let have = &tool.annotations;
                let same = |w: Option<bool>, h: Option<bool>| w.is_none_or(|w| h == Some(w));
                same(want.read_only, have.read_only)
                    && same(want.destructive, have.destructive)
                    && same(want.idempotent, have.idempotent)
                    && same(want.open_world, have.open_world)
            }
        }
    }

    /// The literal tool name this matcher names, if it names exactly one (no wildcards).
    pub fn literal(&self) -> Option<&str> {
        match self {
            Matcher::Glob { pattern, .. } if !pattern.contains(['*', '?', '[', '{']) => Some(pattern),
            _ => None,
        }
    }
}

/// Rules for one server, on the proxy side or in a profile.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    /// Empty means every tool.
    pub include: Vec<Matcher>,
    pub exclude: Vec<Matcher>,
    /// Original name -> new name.
    pub rename: BTreeMap<String, String>,
    /// Original name -> description shown instead of the server's.
    pub describe: BTreeMap<String, String>,
}

impl Rules {
    pub fn visible(&self, tool: &Tool) -> bool {
        let included = self.include.is_empty() || self.include.iter().any(|m| m.matches(tool));
        included && !self.exclude.iter().any(|m| m.matches(tool))
    }
}

/// One server's input to the catalog.
pub struct Server<'a> {
    pub name: &'a str,
    /// Prepended as `<prefix><separator><name>`; empty for no prefix.
    pub prefix: &'a str,
    pub rules: &'a Rules,
    pub tools: &'a [Tool],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exposed {
    pub name: String,
    pub server: String,
    pub original: String,
    pub description: Option<String>,
}

/// Every tool a client sees, sorted by exposed name, with the way back to the server.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    tools: Vec<Exposed>,
    index: BTreeMap<String, usize>,
}

impl Catalog {
    /// `profile`: `None` exposes every server; `Some` only the servers it lists, filtered by
    /// their rules.
    pub fn build(
        servers: &[Server<'_>],
        profile: Option<&BTreeMap<String, Rules>>,
        separator: &str,
    ) -> Result<Catalog, FilterError> {
        let mut tools = Vec::new();
        for server in servers {
            let client = match profile {
                None => None,
                Some(p) => match p.get(server.name) {
                    Some(rules) => Some(rules),
                    None => continue,
                },
            };
            for tool in server.tools {
                if !server.rules.visible(tool) || !client.is_none_or(|c| c.visible(tool)) {
                    continue;
                }
                let original = tool.name.as_str();
                let name = match client.and_then(|c| c.rename.get(original)) {
                    Some(verbatim) => verbatim.clone(),
                    None => {
                        let base = server.rules.rename.get(original).map_or(original, String::as_str);
                        if server.prefix.is_empty() {
                            base.to_string()
                        } else {
                            format!("{}{separator}{base}", server.prefix)
                        }
                    }
                };
                let description = client
                    .and_then(|c| c.describe.get(original))
                    .or_else(|| server.rules.describe.get(original))
                    .cloned()
                    .or_else(|| tool.description.clone());
                tools.push(Exposed { name, server: server.name.to_string(), original: original.to_string(), description });
            }
        }
        let mut index = BTreeMap::new();
        for (i, exposed) in tools.iter().enumerate() {
            if let Some(&first) = index.get(&exposed.name) {
                let first: &Exposed = &tools[first];
                return Err(FilterError::Collision {
                    name: exposed.name.clone(),
                    first_server: first.server.clone(),
                    first_tool: first.original.clone(),
                    second_server: exposed.server.clone(),
                    second_tool: exposed.original.clone(),
                });
            }
            index.insert(exposed.name.clone(), i);
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        let index = tools.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
        Ok(Catalog { tools, index })
    }

    pub fn tools(&self) -> &[Exposed] {
        &self.tools
    }

    /// `(server, original tool)` for an exposed name; `None` for anything not exposed.
    pub fn resolve(&self, exposed: &str) -> Option<(&str, &str)> {
        self.index.get(exposed).map(|&i| (self.tools[i].server.as_str(), self.tools[i].original.as_str()))
    }
}

/// Names that `rules` refer to literally (rename, describe, literal include/exclude)
/// but that the server no longer offers. Reported, ignored at runtime.
pub fn stale(rules: &Rules, tools: &[Tool]) -> Vec<String> {
    let offered: std::collections::BTreeSet<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    let named = rules
        .include
        .iter()
        .chain(&rules.exclude)
        .filter_map(Matcher::literal)
        .chain(rules.rename.keys().map(String::as_str))
        .chain(rules.describe.keys().map(String::as_str));
    let gone: std::collections::BTreeSet<&str> = named.filter(|n| !offered.contains(n)).collect();
    gone.into_iter().map(str::to_string).collect()
}
