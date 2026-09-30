# Alve MVP roadmap

This roadmap describes intended work and acceptance evidence. It does not claim that security, sync, or platform support is already implemented.

## Phase 0 — Product prototype

Create documentation and a non-functional UI prototype that explains local ownership, reviewed AI proposals, graph relationships, foreground device sync, and recovery. Use it to test comprehension with prospective users.

Exit criteria: users can explain that Alve is a curated personal memory graph, that AI proposes rather than silently writes, and that sync is direct and user-started. Capture open questions on desktop and phone platform selection.

## Phase 1 — MVP foundations

Select one desktop and one phone platform based on prototype feedback, local-model feasibility, and peer-to-peer networking support. Define the portable data format, encryption/key lifecycle, device pairing flow, backup format, and user-visible provenance model before feature build-out.

Exit criteria: written threat model, data lifecycle, recovery story, and compatibility constraints are reviewed; exports have a stable documented schema; the platform decision is recorded.

## Phase 2 — Single-device memory MVP

Build local encrypted storage and the essential curation experience: create and edit memories, typed facts and relations, source references, confirmed versus AI-proposed state, search/recall, and reviewed merge/dedup/conflict suggestions. Integrate a local, user-controlled API and an optional MCP adapter; support direct connection to a user-selected model without external fallback.

Exit criteria: a user can operate offline, curate a small memory graph, distinguish proposals from confirmed facts, and export plus restore it on the same device.

## Phase 3 — Two-device foreground sync

Add explicit pairing and user-started sync between the selected desktop and phone over a LAN or phone hotspot. Show sync progress, additions, conflicts, and retained records. Preserve a recoverable history sufficient to avoid automatic deletion during reconciliation.

Exit criteria: two devices exchange a representative dataset using each supported local network path, and the user can resolve duplicates or conflicts before changes are finalized.

## Phase 4 — Recovery and pilot

Harden backup, restore, exports, onboarding, and failure messages. Run a small opt-in pilot with people who use more than one personal device. Collect evidence about curation quality, recall usefulness, sync comprehension, recovery confidence, and local-AI performance.

Exit criteria: pilot participants can recover from a simulated device loss using a backup or open export, and known defects, unsupported environments, and privacy limitations are documented for release planning.

## Required fault tests before any MVP release

- Interrupt sync by disabling Wi-Fi, leaving hotspot range, or closing either app; verify neither device loses confirmed memories and the next foreground sync reports its state clearly.
- Create the same memory independently on both devices, then create conflicting edits; verify the conflict is visible and no version is silently deleted.
- Attempt to open a database with a wrong or unavailable key; verify the app does not reveal protected content and offers an accurate recovery path.
- Restore an older backup over a newer dataset in a test environment; verify the user sees the consequence and can retain or recover newer data according to the documented policy.
- Export a dataset, inspect it with a non-Alve tool, import/restore it in a clean test profile, and verify headings, typed facts, relations, provenance, and proposal status remain intelligible.
- Run capture and recall without internet access using the supported local model path; verify the app does not contact an external model or present one as an automatic fallback.

## Rollout gates

1. **Prototype review:** approve language, workflows, and platform-selection criteria; no security or sync claims.
2. **Internal alpha:** test one desktop and one phone platform with synthetic and non-sensitive data; run all fault tests repeatedly.
3. **Opt-in private beta:** invite a small group, require backup setup, disclose unsupported cases, and collect user-submitted feedback without automatic telemetry.
4. **MVP release decision:** proceed only when fault-test results, export interoperability, recovery outcomes, and pilot feedback meet the success criteria in `product.md`.

The MVP deliberately excludes background sync, mandatory accounts, cloud-hosted memory, autonomous AI writes, and automatic deletion during merge or conflict handling.
