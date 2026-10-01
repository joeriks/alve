# Agent onboarding

## Implementation plan

1. Store portable introductions using the existing encrypted, revisioned memory format.
2. Add a minimal Agents menu with searchable descriptions and resumable onboarding.
3. Review the exact introduction before saving it. Ask an external AI to explain its understanding; record and confirm that response separately.
4. Hand off the first bounded assignment with explicitly selected context. Continue using scoped MCP access and the existing proposal approval workflow.
5. Verify persistence, recovery, stale revisions and disclosure boundaries before introducing automatic execution.

## Using an agent

Open **Menu > Agents > Add agent**. Describe its mission, working method, when to escalate or stay quiet, first assignment and optional manual follow-up date. Select the relevant projects and memories. One agent can cover multiple projects; an introduction supports up to 20 context memories using the existing per-node fact limit. The number of projects in the vault is not limited to 20.

Review and save the introduction. Choose **Actions > Prepare introduction briefing** and copy it to your chosen AI, using **Connect an AI** to configure a scoped MCP connection if needed. Copying is an explicit disclosure of the selected context to the clipboard; send it only to an AI you intend to share it with. Related graph nodes are not automatically included. No token or connection is created by onboarding.

Ask the AI to return its understanding, including missing information. Paste that response into Alve, correct it if necessary, and confirm that it accurately represents the assignment. The app records an owner-confirmed understanding, not proof of the AI's comprehension. Hand off the first assignment after confirmation. Changes to the introduction require a new confirmation.

Agents are ordinary `memory` / `record` nodes marked with `alve-agent`. The mission is the readable body; structured `agent_*` facts retain instructions, phase, understanding, context node IDs and follow-up date. Context IDs are logical links to existing graph nodes. Dates use date facts. Revision checks protect concurrent edits; unresolved conflicts must be resolved before sharing a briefing. Existing unrelated facts and references survive editing.

The description and confirmed understanding are synced as memory content and included in encrypted backups. AI grants remain per-device. Instructions and context selections never confer API permissions. Select and maintain allowed memories separately in **Connections**. Expanding the introduction does not expand any grant.

## AI-initiated assignments and handoffs

The implementation now includes `list_agent_assignments`, `get_agent_briefing`, `get_agent_run`, `prepare_agent_report` and `submit_agent_report` in both MCP bridges. The runtime and authorization live in the Rust core; Python remains an interoperability reference.

Create an AI connection with **read**, **propose** and explicitly enabled **Agent runs (run)**. Select the agent and every memory in its introduction. Existing connections do not gain this permission. Ordinary node search/read scopes do not expand. The new permission additionally permits agent-specific retrieval of approved handoffs whose complete context is within the shared assignment.

Tell your AI: **“Check what Alve needs done.”** The tool descriptions, initialization instructions, usage contract and every briefing explain how to list assignments, retrieve the current assignment and context, and report back. Listings show due dates, changed sources, running leases, interrupted work and reports awaiting approval. Pagination supports many assignments. A briefing requires a ready introduction and returns the latest eligible approved handoff.

`get_agent_briefing` starts a one-hour local lease and accepts a caller-generated `requestId`. Retry the same request with the same ID to recover the same briefing. A different run cannot claim the agent while a local lease or pending report exists. Expired, rejected and abandoned work is never counted as complete. Owner controls are in **Menu > Agent runs**.

Before ending, the AI reports `workPerformed`, `result`, `uncertainties`, `remaining`, `nextAction`, `outcome` (`completed`, `partial`, `blocked`), `nextFollowUp` (timestamp with UTC offset), and references. Each text field is at most 300 characters; the complete generated report is limited to 300 words and 2,000 characters. Alve injects run identity, timestamps and the complete frozen context; callers cannot narrow the report scope.

Report preparation returns the exact memory preview. The AI must ask for human confirmation before submission. **No unattended run may invent that confirmation.** Submission atomically records one pending report, and **Agent runs > Approve handoff** creates the portable report memory. This approves a reported handoff, not proof of payment or completion of a project. Project facts must be changed through their separate proposal workflow.

Preparation/submission and owner approval recheck permissions, revocation, scope and frozen revisions. If the source changed, reject or abandon the old run and request a new briefing. Two prepared report tokens cannot create two pending reports. Ordinary proposal routes cannot submit or approve agent-run reports. Failed storage leaves state and confirmation tokens retryable. `get_agent_run` recovers status after an uncertain submission response.

Approved handoffs are ordinary encrypted memory records tagged `alve-agent-report`, with typed times and server-generated scope facts. They are included in existing backups/manual exchange and can be used after restore. Historical reports are included only if all their context belongs to the new briefing; removed context is never carried into a narrower report. Conflicted or malformed handoffs are excluded. Local leases and pending execution metadata are not exported or synchronized, and do not coordinate simultaneous execution across devices. The local ledger holds at most 200 runs and never silently discards history. **Agent runs > Actions > Clear finished local runs** explicitly removes terminal or expired local execution records while retaining active runs, pending reports and all approved handoff memories.

## Current execution boundary

AI runs in your chosen external client. Alve coordinates assignments and reported handoffs but does not call a model, verify payment, or schedule background checks. Follow-up dates make an assignment eligible for another client-initiated review. Do not infer that a task ran because an agent is ready. AI changes still require exact-preview confirmation and owner approval in Alve.

Next: structured project commitments and an attention overview, then explicit model configuration if app-initiated inference is wanted. Add durable scheduling only after defining missed-run behavior, locked-vault behavior and execution-device ownership.

## Validation

Run `npm run check`, `npm run test:agents` and `python -m unittest discover -s tests -v`. Native interoperability tests use `ALVE_CORE_CHECK` pointing at a compiled acceptance driver.

The model tests check exact selected-context disclosure, missing/archived/conflicted context, limits, structured dates and preservation of unrelated facts. Python and native interoperability tests cover introduction and confirmation persistence, stale edits, recovery and unchanged AI scopes. Browser checks with synthetic data cover the complete onboarding, reload/resume, context search, rebriefing, narrow layout, unsaved review protection, failed-save retries and lock cleanup.

These checks do not validate model quality or autonomous execution. Neither is performed by this version.
