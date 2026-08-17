# Documentation Rules

## Placement

One folder per phase — `phase0/` … `phase4/`. A review goes in the folder of the
phase that produced it (example: `p2-00-review.md` in `phase2/`); soaks and non-task
reviews follow the phase they were captured during.

## Style

Documentation is an index, **not a book**.
Document only information that cannot be learned by reading the code.

Prefer:

- tables over prose;
- bullet lists over paragraphs;
- facts over explanations.

Maximum: **100–300 lines** per document.

Required structure:

1. Summary (5–10 lines)
2. Decisions (max 5 bullets)
3. Bugs found (only if any)
4. Measurements (tables only)
5. Files changed
6. Remaining TODOs

Do NOT:

- explain implementation line-by-line;
- repeat code behavior;
- narrate the development process;
- include historical commentary unless it affected the design;
- write motivational or conversational text;
- write long paragraphs (maximum 5 lines each).

Every sentence must answer a question that cannot be answered by simply reading the source code.

When in doubt, write less.

Do not restate information already present in another project document.
Link to the existing document instead.
