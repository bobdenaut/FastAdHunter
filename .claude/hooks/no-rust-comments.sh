#!/bin/sh
# Blocks Write/Edit/MultiEdit calls that introduce comments into .rs files.
# Enforces CLAUDE.md hard rule 20 mechanically instead of on trust.
#
# Catches line comments (// /// //! //) and block-comment openers (/*), both at
# line start and trailing after code. String and char literals are stripped
# before the scan so a URL inside "https://..." is not a false positive.

input=$(cat)

tool=$(printf '%s' "$input" | jq -r '.tool_name // empty')
case "$tool" in
  Write|Edit|MultiEdit) ;;
  *) exit 0 ;;
esac

path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty')
case "$path" in
  *.rs) ;;
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
  echo "BLOCKED: this edit adds Rust comments to $path" >&2
  echo "$offenders" >&2
  echo "Adding comments into Rust code is forbidden." >&2
  echo "Rewrite the change with zero comment lines and retry." >&2
  exit 2
fi

exit 0
