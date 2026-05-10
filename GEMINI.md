See AGENTS.md

---

## AI Skills & Personas
This project uses a unified set of AI skills and personas shared between Claude Code and Gemini CLI.

### Available Skills
- **`miri-qa`**: Run a production-readiness QA pass. Use when a feature is ready for review.
- **`miri-task`**: Execute a task end-to-end with TDD and full verification.
- **`miri-explorer`**: Fast codebase exploration (Persona: Explorer).
- **`miri-reviewer`**: Independent adversarial review (Persona: Reviewer).
- **`miri-test-runner`**: Run the verification gate (Persona: Test Runner).

### Usage in Gemini CLI
To use these skills, activate them using the `activate_skill` tool or the `/skills activate <name>` command in interactive mode.
- For exploration: `activate_skill("miri-explorer")`
- For task implementation: `activate_skill("miri-task")`
- For QA: `activate_skill("miri-qa")`

### Universal Definitions
The definitions for these skills are located in `.claude/skills/` and `.claude/agents/`. Gemini-specific links and adaptations are maintained in `.gemini/`. 
- **Models**: Equivalent Gemini models are defined via the `gemini-model` field in the skill/agent frontmatter (`flash` for fast tasks, `pro` for complex tasks).