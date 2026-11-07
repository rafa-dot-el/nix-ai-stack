# Every nixpkgs node in flake.lock must be one of the root's channels
# (inputs.nixpkgs / inputs.nixpkgs-unstable); anything else is an input that
# does not `follows` (ADR-0001). Prints one line per violation:
#   <node> (<ref>) <- <parents>
. as $lock
| $lock.nodes as $n
| ($n[$lock.root].inputs // {}) as $root
| [$root.nixpkgs, $root["nixpkgs-unstable"]] | map(select(type == "string")) as $allowed
| $n
| to_entries[]
| select((.value.locked.repo // "") == "nixpkgs"
         and ((.value.locked.owner // "") | ascii_downcase) == "nixos")
| .key
| select(IN($allowed[]) | not) as $bad
| [$n | to_entries[] | select((.value.inputs // {}) | to_entries | any(.value == $bad)) | .key] as $parents
| "\($bad) (\($n[$bad].original.ref // "?")) <- \($parents | join(","))"
