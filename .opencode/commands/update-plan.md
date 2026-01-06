# /update-plan <issue-number>

Update an existing plan with progress. Always includes a continuation prompt.

## Procedure

1. **Fetch current state** - `gh issue view <issue-number>`
2. **Summarize progress** - What was completed, what changed, any blockers
3. **Post update comment** with this format:

   ```markdown
   ## Progress Update - <date>

   ### Completed
   - [x] Task 1
   - [x] Task 2

   ### In Progress
   - [ ] Task 3 (current state: ...)

   ### Blockers / Changes
   - <any issues or scope changes>

   ### Modified Files
   - `path/to/file.rs` - <what changed>

   ---

   ## Continuation Prompt

   To continue this work in a new session, use:

   > Continue work on #<issue-number>: <title>
   >
   > Current state: <brief status>
   > Next step: <specific next action>
   > Key files: `file1.rs`, `file2.rs`
   ```

4. **Update labels** - Add/remove `in-progress` as appropriate
5. **Update checkboxes** - Edit issue body if tasks completed: `gh issue edit <number> --body-file`
