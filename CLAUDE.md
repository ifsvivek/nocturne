# NOCTURNE — Agent Orchestration, Git and Skills Policy

## 1. Agent Hierarchy

The MAIN AGENT is the project orchestrator and technical supervisor.

Use Claude Sonnet 5.5 as the main agent, provided it is available in the configured environment.

The main agent's primary responsibilities are:

- Understand requirements.
- Inspect the repository.
- Plan and decompose tasks.
- Assign implementation tasks to subagents.
- Coordinate dependencies between tasks.
- Review and validate subagent implementations.
- Identify bugs and request corrections.
- Integrate completed work.
- Verify tests and functionality.
- Maintain architectural consistency.
- Manage project progress and documentation.

### Main Agent — Management Only

The main agent should NOT routinely implement application features itself.

Its primary job is to:
1. Determine what needs to be done.
2. Break work into well-defined tasks.
3. Assign those tasks to subagents.
4. Monitor implementation progress.
5. Inspect and validate their output.
6. Request corrections where necessary.
7. Approve completed work.

The main agent must retain responsibility for technical decisions and final acceptance.

### Subagents — Implementation Team

You may create a maximum of TWO subagents.

Both subagents MUST use Claude Sonnet 5.5 when that model is available.

Subagents are the primary code-writing agents.

They are responsible for:
- Writing Rust code.
- Implementing GPUI components.
- Implementing Jellyfin integration.
- Implementing libmpv integration.
- Building application features.
- Writing tests.
- Fixing implementation bugs.
- Updating relevant documentation.

The main agent should delegate actual implementation work rather than performing it itself.

### Delegation Constraints

- Never exceed two active subagents.
- Prefer one subagent when work can be completed sequentially.
- Use two only when tasks are genuinely independent.
- Never create additional agents to increase speed.
- Never assign overlapping file modifications.
- Give each subagent a clearly defined task and expected result.
- Require implementation summaries and test results.
- The main agent must review all completed work.
- Subagents must not independently redefine the project architecture.
- Do not repeatedly delegate the same investigation.
- Avoid unnecessary agent conversations and handoffs.

If a subagent encounters an architectural problem, it must report the issue to the main agent instead of independently making major architectural changes.

## 2. Resource Conservation

I explicitly prioritize low Claude Code usage over speed.

I want slow, deliberate, efficient engineering.

The goal is to minimize unnecessary usage across my Claude Code account.

Rules:

- Keep the main agent focused on orchestration and validation.
- Keep subagent tasks narrowly scoped.
- Do not spawn agents for trivial tasks.
- Avoid unnecessary parallelism.
- Avoid redundant research.
- Reuse existing findings and documentation.
- Avoid repeated repository-wide inspections.
- Avoid repeated identical tests.
- Avoid unnecessary model switching.
- Do not produce excessive progress reports.
- Do not perform work simply to appear productive.

Use the minimum number of agent calls necessary to produce reliable results.

Do not sacrifice correctness to reduce usage, but do not waste usage either.

## 3. Git and Private Repository Setup

Set up Git properly for NOCTURNE.

Inspect the existing repository before making changes.

If Git is not initialized:
- Initialize Git.
- Create an appropriate .gitignore.
- Configure a sensible initial branch.
- Review the initial project files.

### GitHub Integration

Use my authenticated GitHub account to create a repository for NOCTURNE.

Requirements:
- Repository visibility MUST be PRIVATE.
- Use `nocturne` as the repository name unless it is already occupied.
- Push the project to my GitHub account.
- Configure the appropriate remote.
- Establish a clean initial commit.
- Keep the repository synchronized as development progresses.

First inspect whether GitHub CLI (`gh`) is installed and authenticated.

If authentication is missing, ask me to complete authentication rather than requesting or storing credentials.

Before pushing:
- Check for secrets.
- Check for local configuration containing credentials.
- Ensure access tokens and passwords are excluded.
- Review .gitignore.
- Never upload private Jellyfin server credentials or media information.
- Never upload personal files unrelated to NOCTURNE.

Do not make the repository public.

Do not overwrite an existing remote repository or unrelated project.

If a repository already exists, inspect it and determine the safest way to proceed.

### Git Workflow

Use meaningful commits describing actual changes.

Keep commits focused.

Do not commit unfinished secrets or generated build artifacts.

The main agent is responsible for validating changes before they are committed and pushed.

## 4. Skills Discovery and Usage

Before implementing substantial work, inspect the skills available in the Claude Code environment.

### Design and Motion Skills

Discover and load all relevant installed skills related to:

- Product design.
- UI/UX design.
- Visual design.
- Design systems.
- Interface implementation.
- Animation.
- Motion design.
- Interaction design.
- Accessibility.
- Design review and QA.

Load the relevant skill instructions before starting tasks in those areas.

Do not assume a skill exists just because it is mentioned here.

Use available skills when they materially improve the implementation.

### Ponytail Skill

Specifically check whether a skill named `ponytail` or `Ponytail` is installed.

If available:
- Read its complete instructions.
- Understand its intended purpose.
- Apply it to relevant project work.
- Follow its requirements.

If unavailable, report that it could not be found. Do not invent its capabilities or silently substitute another skill.

### Skill Usage Rules

- Load skills when relevant to the task.
- Avoid repeatedly loading unchanged instructions.
- Reuse knowledge from previously loaded skills.
- Do not load unrelated skills.
- Follow the actual installed skill documentation.
- Do not allow skills to override project safety or repository requirements.

## 5. Final Responsibility Model

MAIN AGENT:
Plan → Delegate → Coordinate → Review → Validate → Approve.

SUBAGENTS:
Receive tasks → Research as needed → Write code → Test → Report results.

The main agent is the technical supervisor, not the primary feature implementer.

The subagents are the implementation team.

Maintain this division throughout NOCTURNE development.