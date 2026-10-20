---
title: "PRD: Impulse, Ion, and Photon"
description: Product requirements from James for the application and meta harness (Impulse), its owned coding agent (Ion), and typed Photon cards
updated: 2026-10-03
type: specification
category: product
phase: all
status: active
audience: builders
tags: [prd, impulse, ion, photon, session, north-star]
---

# PRD: Impulse, Ion, and Photon

> Source: James, `impulse-ion-photon-prd.docx`, received 2026-10-03. Converted to Markdown
> verbatim (text and tables); the `.docx` remains the original. Implementation decisions taken
> against it are recorded in `docs/plans/worktrees/2026-10-02-claude-ion-photon-subagent.md`.


The application, the meta harness inside it, the coding agent it owns, and a reader that returns a filled result.

Status: draft for build. Audience: the person writing the code. Scope: one repository, two processes.

## 1. Purpose

Impulse is the whole application, and it is the meta harness inside that application. One name, one package, two roles. The application is the program a person opens. The meta harness sits around a working agent, observes it, and helps maintain the session. Ion is the coding agent Impulse starts when the session needs a working agent it owns. Photon is a named reader that working agent calls inside one tool call. Claude and Codex can occupy a session and keep the loops they already have. The same meta harness observes them and helps maintain their sessions. ROSA remains a separate application, with its own process and its own approval record.

A session is complete when a person can start it, watch a claim arrive, see verify run on the claimed commit, record acceptance, and find one pending memory note on disk. Photon is complete when Ion can call two cards and receive either a filled result or an unfilled result.

## 2. Terms

Each word below has one meaning in this document. The command names stay governed-claim, governed-verify, and governed-review, and they match claim, verify, and review.

| Term | Meaning |
|---|---|
| Impulse | The whole application, and the meta harness inside it. The application is what a person opens. The meta harness observes the working agent and helps maintain the session. |
| Ion | The coding agent Impulse owns. It reads, writes, runs commands, submits a claim, and calls Photon. |
| Photon | A named reader Ion calls inside one tool call. It returns a filled result or an unfilled result. |
| Session | One task, one workspace, one working agent, and the record Impulse writes for that task. |
| Working agent | The process doing the coding in the session. Ion, Claude, or Codex. |
| Pane | The terminal view of the working agent. Closing the pane leaves the session record in place. |
| Confirmation | The person's allow, before the act, on a write, a command, or a claim submission. |
| Claim | The working agent's statement of what it did, plus the artifact ids. Impulse attaches the agent name and the commit. Command: governed-claim. |
| Verify | The commands Impulse runs in a separate checkout of the claimed commit. Command: governed-verify. |
| Review | One model call with no tools. It returns a verdict bound to the task revision, the claim, the verify, and the criteria. Command: governed-review. |
| Acceptance | The person's final decision on the task, after review. Accept or reject. |
| Memory note | The pending file Impulse writes after acceptance. The person keeps it or drops it. Until kept, it stays out of the project memory file. |
| Photon card | One named Photon call. Fixed input, fixed output, lookup list, and limit. |
| Filled result | A Photon output that matches the card. not_found is a filled result of find_symbol. |
| Unfilled result | A Photon call that ended without a valid output. The reason is time, bad_reply, provider, or trace. |

## 3. Two roles, one name

Impulse is both. The roles share a process and a package. They are named apart so a sentence can say which job is being done.

### 3.1 The application

The application is the program a person opens. It includes the long-running process, the terminal, the pane, the session directory, and the working agents it starts. A person registers a repository, types a task and criteria, gives confirmation, records acceptance, and keeps or drops the memory note through the application.

### 3.2 The meta harness

The meta harness is the role that sits around a working agent. It observes the agent and helps maintain the work. Observation is the record. Maintenance is verify, the memory note, and the context a later session receives.

| Observe | Maintain |
|---|---|
| Start of the working agent, and the workspace it was given. | Bind the session to the repository and the clean base commit. A dirty tree stays closed. |
| Claim statement and artifact ids the working agent sends. | Attach the agent name and the commit Impulse observed. Write the claim line. |
| Exit of the working agent, with or without a claim. | Close the session unclaimed when no claim line exists. |
| The commit named on the claim line. | Run verify in a detached checkout of that commit. Write the verify line and the log. |
| The review verdict, bound to the task, the claim, the verify, and the criteria. | Store the verdict. Wait for the person to record acceptance. |
| Acceptance or rejection. | Write the acceptance line. On acceptance, write the pending memory note. |
| A kept memory note. | Hand that note into a later session as context. A dropped note stays out. |

The working agent submits the claim. Impulse writes the acceptance. A Photon call is observed as a tool result. It leaves the session files unchanged, and it leaves confirmation in place for a later write, command, or claim.

### 3.3 What the meta harness leaves to the working agent

Ion reads, writes, runs commands, and calls Photon. Claude and Codex keep their own loops. The meta harness starts them, watches them, and writes the session record. Their internal loops stay in their own programs.

## 4. Who acts

| Actor | May do | Writes |
|---|---|---|
| Person | Register a repo, type the task and criteria, give confirmation, record acceptance, keep or drop the memory note. | Acceptance, through Impulse. |
| Impulse | Start the working agent, record the base commit, receive the claim, run verify, store the review, write the memory note. | session.json, claim.jsonl, verify.jsonl, accept.jsonl, note.md. |
| Ion | Read, write, run a command, submit a claim, call a Photon card. | Workspace files, after confirmation. The claim statement. |
| Guest working agent | Claude or Codex, in a session Impulse started, keeping its own loop. | Its own workspace changes. Impulse records start, exit, claim, verify, and acceptance. |
| Photon | Fill one card from workspace reads, or return an unfilled result. | Nothing on disk. The return value stays inside the call. |

## 5. Session states

A session is in one state. Impulse is the process that changes it.

| State | Entered when | Leaves when |
|---|---|---|
| open | session.json is written and the base commit is recorded. | The worker process has started. |
| running | The worker is alive in the workspace. | A claim line is appended, or the worker exits with no claim. |
| claimed | A claim line exists for the current commit. | Verify finishes. |
| verified | A verify line exists. A pass moves to review. A fail stays verified, and a new claim can follow a new commit. | Review returns, on a pass. |
| in_review | A review verdict names this claim and this verify. | The person records acceptance or rejection. |
| accepted | An acceptance line says yes, and note.md exists as pending. | The person keeps or drops the memory note. The task decision stays. |
| rejected | An acceptance line says no. | A new session. This task stays rejected. |
| closed_unclaimed | The working agent exited with no claim line. | A new session. |

A Photon call leaves the session state where it was. The session files are the same before and after the call.

## 6. Records

The first record is one directory per session. A person can read it with ordinary file tools.

### 6.1 session.json

| Field | Rule |
|---|---|
| session_id | Unique. Present on every later line. |
| project_id | The registered repository. |
| workspace | Absolute path of the working tree the worker sees. |
| base_commit | Clean commit recorded before the worker starts. A dirty tree refuses to open. |
| worker | ion, claude, or codex. |
| started_at | When Impulse wrote the file. |
| state | One of the states in section 5. |

### 6.2 claim.jsonl

One line per claim. The working agent sends the statement and the artifact ids. Impulse writes the line and fills the commit and the agent name.

| Field | Rule |
|---|---|
| session_id | Must match session.json. |
| summary | Short text from the worker. Bound in length. |
| commit | Clean commit Impulse observed at claim time. |
| artifact_ids | Ids the worker names. Empty is allowed. |
| worker | Copied from the session. The working agent does not set this. |

### 6.3 verify.jsonl

Verify runs in a detached checkout of the claimed commit. The working agent's folder is left alone. For a Rust workspace the commands are format, lockfile check, Clippy, and tests. Passing requires a clean tree before and after.

| Field | Rule |
|---|---|
| session_id | Must match. |
| commit | The commit on the claim line being verified. |
| commands | The list that ran, in order. |
| pass | True only if every command passed and both trees were clean. |
| log_path | Path to the verify log, outside the working agent's workspace. |

### 6.4 accept.jsonl and note.md

Review is one model call with no tools. It names the task revision, the claim, the check, and the criteria. The person then records acceptance or rejection. That decision is final for the task.

| Field | Rule |
|---|---|
| session_id | Must match. |
| decision | accepted or rejected. |
| note_id | Set on accept. Empty on reject. |

note.md lists the task, the criteria, the commit, and the verify commands that passed. It leaves out the claim statement and the review reasons. Status is pending until the person keeps or drops it. Until then it stays out of the project memory file.

## 7. Confirmation

These wait for the person: file_write, bash_exec, governed-claim. These do not: file_read, document_read, memory_search, genome_read, ion_verify, explain_error, find_symbol.

A Photon result waits with the later act. A write, a command, or a claim that follows a Photon call still asks for confirmation.

governed-claim, governed-verify, and governed-review stay the command names. file_read, document_read, file_write, and bash_exec stay the tool names. document_read remains Ion's reader for spreadsheets, Word files, and PDFs.

## 8. Photon contract

A card has a name, input fields, output fields, a lookup list, a round limit, a call limit, and a second limit. Ion calls it the way it calls any other tool. Photon may loop while it fills the output. It reads the workspace through its own lookups. It returns a filled result, or an unfilled result. Ion receives the card. The search transcript stays inside the call.

### 8.1 Unfilled result

| Reason | Meaning |
|---|---|
| time | Round, call, or second limit reached before a filled result. |
| bad_reply | The model reply omitted a field, or the output failed to parse. |
| provider | The model call failed. |
| trace | The call record could not be written. |
| not_found | A filled result of find_symbol. The name is absent. |

### 8.2 Path rules

A path must sit inside the workspace. Absolute paths, parent escapes, symlinks, .git, target, node_modules, and credential filenames are refused. Hard-linked file contents are refused. Known key shapes in file text are masked before the model sees them.

Read limits, copied from the small standalone reader: directory listings stop at 5,000 entries, a read returns at most 8,192 bytes on a character boundary, and search stays literal and depth-bounded. That standalone reader remains a program a person can run with Impulse stopped. Impulse copies the limits.

### 8.3 Cards

| Card | Input | Output | Lookups | Limit |
|---|---|---|---|---|
| explain_error | command, exit_code, stderr (bounded) | cause, file, line, next_check | none | one model call |
| find_symbol | name | path, line, note, or not_found | list, read | 2 passes, 4 reads, 20 seconds |

find_symbol calls no other Photon card. A lookup outside list and read fails the catalog test.

## 9. Alternative specs

Each pair is a real build choice. The design sequence follows the recommended path.

### 9.1 Session record

| Spec | Shape | Depends on |
|---|---|---|
| A, files | One directory per session. A person can read a single run. | Nothing else in this document. |
| B, table | The same four events in SQLite. note.md rendered from the accept row. Easier to list across sessions. | Spec A, and a second session that needs a list. |

### 9.2 Photon surface

| Spec | Shape | Depends on |
|---|---|---|
| A, one tool per card | explain_error and find_symbol sit beside file_read. The tool list matches the names a person can call. | A tool slot on Ion. |
| B, catalog | Each card is a struct: name, input fields, output fields, lookups, round limit, call limit, seconds. A register line exposes it. A test requires the lookup list to be a subset of list and read. | Spec A, because the second card is what the register line proves. |
| C, one tool | A single tool named photon takes a card field. The handler selects the schema. The registry stays short. The tool list no longer shows the card names. | The catalog, and a further card that makes the list long. |

### 9.3 Photon process

| Spec | Shape | Depends on |
|---|---|---|
| S, function | Photon runs inside the Ion process. Ion passes the input. Photon returns a filled result or an unfilled result. The session list shows Ion. | The card contract. |
| T, helper process | Ion starts a short-lived process, passes the question and the workspace root on standard input, and reads one JSON object from standard output. Impulse does not list that process as a session. A Photon panic leaves Ion running. | Spec S, and a panic that takes Ion down. |

## 10. Invariants

- Impulse writes the acceptance line. The working agent sends the claim statement.

- Verify runs on the commit named in the claim, in a checkout that is not the working agent's folder.

- A dirty tree at session start refuses to open.

- A Photon call leaves session.json and the three logs unchanged.

- A card lookup list is a subset of list and read.

- A card calls no other card.

- A Photon result leaves confirmation in place for a later write, command, or claim.

- The memory note stays out of the project memory file until the person keeps it.

- ROSA is unchanged by this release.

## 11. A finished run

- The person registers the repository and types the task and the criteria.

- Impulse writes session.json and the clean commit, then starts Ion with the session id in the environment.

- Ion edits, runs its own checks, and submits a claim. Impulse appends a claim line.

- Impulse verifies that commit in a detached checkout and appends a verify line.

- Review returns a verdict bound to that claim and that verify.

- The person records acceptance. Impulse appends an acceptance line and writes note.md as a pending memory note.

- During the work, Ion may call explain_error or find_symbol. Each call returns a filled result or an unfilled result. The session files stay unchanged by the call.

## 12. Design sequence

Order here is dependency. A later piece needs the earlier piece in order to be designed and checked.

The session directory comes first. A card is called during a session, and the call must leave the session files unchanged. That comparison needs a session to exist. The session includes claim, verify in a detached checkout, accept, and the pending note. It is complete when one repository has a session directory with all five files, and the accept line points at the note.

explain_error comes next. It needs the session, the model client Ion already has, and a tool slot beside file_read. It needs no catalog. It is complete when a failing command returns a filled result or an unfilled result, and the session files are unchanged.

find_symbol comes after that. The second card needs the catalog, because the catalog is what a second card proves. It also needs list and read lookups, which explain_error does not use. It is complete when a known symbol returns path and line, an unknown symbol returns not_found, and the card cannot call explain_error.

A session table waits on Spec A and a second session. A single photon tool waits on the catalog and a further card. A helper process waits on Spec S and a panic that takes Ion down. Guest workers can be offered the same card names through the existing tool bridge after both cards return a filled result or an unfilled result. They receive a function call, and Impulse still records the session.

## 13. Release checks

- One repository goes from session start to a pending note.

- The verify log is from a detached checkout of the claimed commit.

- A dirty tree at start produces no session.json.

- explain_error returns a filled result or an unfilled result with a named reason.

- find_symbol returns a filled result, including not_found, inside its limit.

- A Photon result leaves confirmation in place for a later write, command, or claim.

- The memory note is absent from the project memory file.

- ROSA is unchanged.
