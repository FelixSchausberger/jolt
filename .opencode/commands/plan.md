# /plan <description>

Create a plan for a feature or task. Produces a GitHub issue as the source of truth.

## Procedure

1. **Research** - Explore codebase to understand scope and constraints
2. **Draft** - Create `./scratchpad/plan-<slug>.md` with:
   - Problem statement
   - Proposed approach
   - Implementation steps (checkboxes)
   - Open questions
3. **Review** - Present draft to user for feedback
4. **Finalize** - Once approved, create GitHub issue:
   ```bash
   gh issue create --title "<title>" --body-file ./scratchpad/plan-<slug>.md --label "feature"
   ```
5. **Cleanup** - Delete scratchpad file after issue is created
