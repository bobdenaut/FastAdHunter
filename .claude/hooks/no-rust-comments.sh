#!/bin/sh
# Blocks Write/Edit/MultiEdit calls that introduce comments into source files:
# .rs, and .ts/.tsx for the dashboard, which had no gate and reached a higher
# comment density than the Rust tree without one.
# Enforces CLAUDE.md's no-comments-in-Rust hard rule mechanically instead of on
# trust. Referenced by name, not by number: this line said "hard rule 20" while
# the rule is 7, because it was written against plan/CLAUDE.md's old numbering.
#
# Catches line comments (// /// //! //) and block-comment openers (/*), both at
# line start and trailing after code. String and char literals are stripped
# before the scan so a URL inside "https://..." is not a false positive, and so
# are single-line template literals: `${scheme}//${host}` is code, not a
# comment.

input=$(cat)

tool=$(printf '%s' "$input" | jq -r '.tool_name // empty')
case "$tool" in
  Write|Edit|MultiEdit) ;;
  *) exit 0 ;;
esac

path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty')
case "$path" in
  *.rs|*.ts|*.tsx) ;;
  *) exit 0 ;;
esac

added=$(printf '%s' "$input" | jq -r '
  [ .tool_input.content?
  , .tool_input.new_string?
  , (.tool_input.edits? // [] | .[].new_string?)
  ] | map(select(. != null)) | join("\n")
')

offenders=$(printf '%s\n' "$added" | awk '
  {
    line = $0
    gsub(/r#*"[^"]*"#*/, "S", line)
    gsub(/`[^`]*`/, "T", line)
    gsub(/"([^"\\]|\\.)*"/, "S", line)
    gsub(/'"'"'([^'"'"'\\]|\\.)*'"'"'/, "C", line)
    if (line ~ /\/\/[ \t]*SAFETY:/) next
    if (line ~ /\/\// || line ~ /\/\*/) {
      printf "  line %d: %s\n", NR, substr($0, 1, 100)
      hit++
    }
  }
  END { exit (hit > 0 ? 1 : 0) }
')

if [ -n "$offenders" ]; then
  echo "BLOCKED: this edit adds comments to $path" >&2
  echo "$offenders" >&2
  echo "Adding comments into source code is forbidden." >&2
  echo "The why belongs in API.md, docs/decisions/, docs/solutions/ or a test name." >&2
  echo "Rewrite the change with zero comment lines and retry." >&2
  exit 2
fi

exit 0
