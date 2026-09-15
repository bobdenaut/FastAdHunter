#!/bin/sh
# Test for no-rust-comments.sh. Run: sh .claude/hooks/no-rust-comments.test.sh
# Every case names the behaviour it pins; a failure prints the case and the
# exits that disagreed.

hook=$(dirname "$0")/no-rust-comments.sh
failed=0

check() {
  want=$1
  name=$2
  json=$3
  printf '%s' "$json" | sh "$hook" >/dev/null 2>&1
  got=$?
  if [ "$got" = "$want" ]; then
    echo "  ok    $name"
  else
    echo "  FAIL  $name (wanted exit $want, got $got)"
    failed=$((failed + 1))
  fi
}

echo "blocked:"
check 2 "a trailing comment in .tsx" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.tsx","new_string":"const x = 1; // why"}}'
check 2 "a JSDoc block written whole into .ts" \
  '{"tool_name":"Write","tool_input":{"file_path":"a.ts","content":"/** shape */\nexport type A = {}"}}'
check 2 "a trailing comment in .rs" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.rs","new_string":"let a = 1; // nope"}}'
check 2 "a doc comment in .rs" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.rs","content":"/// what it does\nfn f() {}"}}'
check 2 "a comment inside a MultiEdit edit" \
  '{"tool_name":"MultiEdit","tool_input":{"file_path":"a.ts","edits":[{"new_string":"const ok = 1;"},{"new_string":"const bad = 2; // here"}]}}'

echo "allowed:"
check 0 "a URL in a template literal" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.ts","new_string":"return `${scheme}//${window.location.host}${path}`;"}}'
check 0 "a URL in a double-quoted string" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.ts","new_string":"const u = \"https://x.test\";"}}'
check 0 "a URL in a Rust raw string" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.rs","new_string":"let u = r#\"https://x.test\"#;"}}'
check 0 "comment-free TypeScript" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.ts","new_string":"export const n = 5;"}}'
check 0 "the SAFETY comment unsafe requires" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.rs","new_string":"// SAFETY: bounds checked above\nunsafe { p.read() }"}}'

echo "out of scope:"
check 0 "a .css file" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.css","new_string":"/* fine */"}}'
check 0 "a .md file" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.md","new_string":"// fine"}}'
check 0 "a .sh file" \
  '{"tool_name":"Edit","tool_input":{"file_path":"a.sh","new_string":"# fine"}}'
check 0 "a tool that does not write" \
  '{"tool_name":"Read","tool_input":{"file_path":"a.rs"}}'

if [ "$failed" -gt 0 ]; then
  echo "$failed case(s) failed"
  exit 1
fi
echo "all cases passed"
