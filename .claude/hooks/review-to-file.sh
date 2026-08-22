#!/bin/sh
# UserPromptSubmit: when the prompt is a code-review request, remind the agent
# that findings belong in docs/code-review/<phase>/<task>-review.md, not chat.
prompt=$(jq -r '.prompt // ""' 2>/dev/null)
case "$prompt" in
  *[Rr]eview*|*[Ff]indings*)
    printf '%s' '{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"CODE REVIEW OUTPUT RULE (plan/CLAUDE.md §CODE REVIEW): findings go ONLY into the task review file under docs/code-review/<phase>/<task>-review.md (## Findings section, severity-ranked, ending with PASS / PASS WITH DEFERRED FINDINGS / BLOCKED). NEVER present findings in chat. The only chat reply is: `Review written to <path>.`"}}'
    ;;
esac
exit 0
