//! Where a message may go and how (plan 5.4): rules in order, the first match wins, the
//! default otherwise. One engine for the CLI and the MCP tools (pure).

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Live,
    Draft,
    Deny,
}

/// What a rule can match on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Match {
    pub self_dm: Option<bool>,
    /// Glob on the channel name (without #).
    pub channel: Option<String>,
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub matches: Match,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub default: Action,
    pub rules: Vec<Rule>,
}

/// A resolved destination, as the policy sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub channel_id: String,
    /// The channel's name, when it has one (DMs do not).
    pub name: Option<String>,
    pub self_dm: bool,
}

/// The action for `dest`, and the index of the rule that decided (None: the default).
pub fn decide(policy: &Policy, dest: &Destination) -> (Action, Option<usize>) {
    let hit = policy.rules.iter().position(|rule| {
        let m = &rule.matches;
        m.self_dm.is_none_or(|v| v == dest.self_dm)
            && m.channel_id.as_ref().is_none_or(|id| *id == dest.channel_id)
            && m.channel.as_ref().is_none_or(|glob| {
                let matcher = globset::Glob::new(glob).map(|g| g.compile_matcher());
                matches!((matcher, &dest.name), (Ok(g), Some(name)) if g.is_match(name))
            })
    });
    match hit {
        Some(i) => (policy.rules[i].action, Some(i)),
        None => (policy.default, None),
    }
}

/// The action a request asked for, checked against the policy's: a request may ask for less
/// (live -> draft) but never more (draft -> live, deny -> anything).
pub fn allowed(policy_action: Action, requested: Option<Action>) -> Result<Action, String> {
    match (policy_action, requested) {
        (Action::Deny, _) => Err("denied by the slack.yaml policy for this destination".into()),
        (_, Some(Action::Deny)) => Err("deny is not a mode to send in".into()),
        (Action::Draft, Some(Action::Live)) => Err("the policy only allows a draft here, not a live post".into()),
        (policy, None) => Ok(policy),
        (_, Some(asked)) => Ok(asked),
    }
}
