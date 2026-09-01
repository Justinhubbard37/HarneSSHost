# HarneSSHost Project Doctrine — CANONICAL

**Version:** 1.0  
**Status:** CANONICAL — DIRECTOR APPROVED  
**Effective Date:** 2026-09-01  
**Purpose:** Enduring product, architecture, engineering, and evaluation doctrine for HarneSSHost.

---

## 1. Purpose and Authority

This document defines the enduring macro-level direction of HarneSSHost.

It establishes:

- what HarneSSHost is;
- what architectural principles must be preserved;
- what product directions have already been chosen;
- what future evolution is intentionally supported;
- what engineering and hygiene standards are non-negotiable.

This document does **not** define:

- implementation phases;
- gate sequencing;
- current branch or repository state;
- temporary machine state;
- individual task instructions;
- exact implementation mechanisms where evidence has not yet selected them.

Those belong in project-state records, gate plans, audits, implementation specifications, and the HarneSSHost Evaluation Standard.

When this doctrine conflicts with temporary implementation convenience, the doctrine takes precedence unless explicitly amended by the Director.

---

## 2. Product Identity

HarneSSHost is a **modular multi-harness desktop host and evaluation platform for agentic AI systems**.

Its foundational model is:

> **Agent = Model + Harness**

The model provides intelligence.

The harness provides the environment through which that intelligence acts, including tools, execution loops, context management, permissions, memory, agents, interfaces, and other runtime capabilities.

HarneSSHost must preserve the ability to treat the **model** and the **harness** as independently selectable and evaluable variables.

HarneSSHost is not intended to replace the harness, silently normalize away its legitimate behavior, or make every harness behave identically.

---

## 3. Modular Harness Architecture

A harness integration is a **module**, not an architectural fork of HarneSSHost.

HarneSSHost core must remain harness-agnostic wherever practical.

Harness-specific behavior belongs behind the harness integration boundary, including as applicable:

- discovery;
- provenance;
- acquisition;
- capabilities;
- runtime topology;
- ownership;
- readiness;
- authentication;
- lifecycle behavior;
- presentation;
- harness-specific evidence.

Adding, replacing, disabling, or removing one harness must not require redesigning unrelated harness integrations or rewriting generic core behavior.

A healthy integration should be removable primarily through its own module and registration boundary rather than through widespread edits across the application.

HarneSSHost must not evolve into a distributed collection of harness-specific conditionals throughout generic core code.

---

## 4. Harness Fidelity

HarneSSHost adapts to legitimate harness differences. It does not erase them.

Different harnesses may legitimately use different:

- operating environments;
- filesystems;
- shells;
- tools;
- planning systems;
- permission models;
- agents or subagents;
- memory systems;
- retry behavior;
- authentication mechanisms;
- runtime ownership mechanisms;
- readiness mechanisms;
- interfaces;
- client-close semantics.

A harness must not be forced into another harness's architecture merely for implementation convenience or artificial uniformity.

New generic abstractions should normally arise from demonstrated integration needs rather than speculative future complexity.

---

## 5. Runtime Profiles

A harness is not necessarily equivalent to one runtime configuration.

**HarneSSHost should run each harness in the developer-intended environment and topology whenever that can be reliably established.**

HarneSSHost shall support the concept of multiple legitimate **runtime profiles** for the same harness where evidence and product needs justify them.

A runtime profile may vary by factors such as:

- operating environment;
- interface;
- execution topology;
- ownership mechanism;
- workspace model;
- connectivity method.

The harness identity and runtime profile must remain distinguishable.

---

## 6. User Experience and Presentation

HarneSSHost supports three presentation paths.

### 6.1 Official Interface

When a suitable authoritative harness interface exists, preserving the official interface is preferred.

This protects harness fidelity and exposes the real user experience, including native tools, permissions, workflows, and operator burden.

### 6.2 HarneSSHost Default Interface

For harnesses without a suitable official interface, HarneSSHost will provide a usable standardized interface.

The current intended implementation direction is **assistant-ui**, or an explicitly approved successor.

This is a fallback capability, not a replacement for authoritative interfaces that should be preserved.

### 6.3 User-Custom Interface

Users may create their own interface for a supported harness.

Custom interfaces are **capability-constrained**.

A user may arrange, expose, hide, or organize supported controls, but may not invent capabilities the selected harness/runtime profile does not possess.

Example:

If a harness does not support Web Search, the UI builder must not offer a Web Search control for that harness.

Customization changes presentation. It does not change capability truth.

---

## 7. Capability Truth

HarneSSHost must never knowingly represent a capability as available when it cannot actually be exercised through the selected harness/runtime profile.

Capability state should distinguish, where relevant:

- native harness capability;
- HarneSSHost-provided capability;
- adapter-provided capability;
- supported capability;
- unsupported capability;
- unknown or unverified capability.

The default interface and custom interface builder must derive available controls from validated capabilities or action bindings rather than arbitrary UI configuration.

HarneSSHost-provided functionality must not be falsely attributed to the harness.

---

## 8. Evaluation Architecture

HarneSSHost supports two distinct evaluation paths.

### 8.1 Controlled Comparison

Purpose:

> Measure differences under deliberately controlled conditions so that the effect of the changed variable can be isolated as rigorously as practical.

Typical uses include:

- same model across different harnesses;
- same harness across different models;
- controlled investigation of the Harness Delta.

Controlled Comparison must not falsify the harness merely to make every participant mechanically identical.

### 8.2 Native Capability

Purpose:

> Measure what the complete agentic system can actually accomplish while using its legitimate native capabilities.

Native functionality may include:

- proprietary tools;
- agents or subagents;
- native planning;
- memory;
- context systems;
- native interfaces;
- specialized workflows.

Native Capability results must not be presented as isolated causal evidence for the harness alone.

### 8.3 Separation of Evaluation and Presentation

Evaluation mode and presentation mode are separate dimensions.

A run may independently identify:

- model;
- harness;
- runtime profile;
- presentation profile;
- evaluation mode;
- task or evaluation contract.

HarneSSHost must preserve those distinctions in its architecture and evidence.

---

## 9. HarneSSHost Evaluation Standard Relationship

The **HarneSSHost Evaluation Standard (HES)** is a separate, evolving evaluation authority.

HarneSSHost should provide the architectural seams and evidence required for rigorous evaluation, including as appropriate:

- provenance;
- run identity;
- capability identity;
- environment identity;
- model-visible state;
- trajectory evidence;
- approval events;
- artifacts;
- outcomes;
- reproducibility information.

The runtime application must not prematurely hard-code provisional HES scoring rules, unfinished benchmark methodology, or temporary evaluation proposals.

HES may evolve independently while HarneSSHost remains capable of supporting it.

---

## 10. Security, Ownership, and Evidence

### Ownership

HarneSSHost manages only workloads it can prove it owns.

It must not silently adopt foreign runtimes based on weak observations such as process name, port number, or circumstantial similarity.

Unrelated workloads must not be terminated by HarneSSHost cleanup.

Ownership and cleanup must be deterministic and independently verifiable.

### Secrets

Secrets must be exposed only where functionally necessary.

Convenience does not justify leaking credentials into:

- frontend state;
- logs;
- audit artifacts;
- command arguments;
- retained URLs;
- diagnostics;
- unrelated configuration.

### Evidence

HarneSSHost development and evaluation must distinguish:

> Implementation ≠ proof  
> Tests ≠ acceptance  
> Builder PASS ≠ Director acceptance  
> Proposal ≠ canonical law  
> Historical evidence ≠ present machine reality

Claims must not exceed the evidence supporting them.

---

## 11. Project and Repository Hygiene

**Exceptional project hygiene is a mandatory architectural requirement of HarneSSHost.**

It is not optional cleanup work and must not be deferred indefinitely for convenience.

HarneSSHost must maintain high standards across:

- Git history;
- branches;
- repository structure;
- naming;
- dependencies;
- source organization;
- documentation;
- tests;
- generated artifacts;
- evidence;
- temporary experiments;
- secrets;
- dead and transitional code.

Required principles include:

- feature work belongs on purpose-appropriate branches;
- obsolete feature branches must not become accidental permanent development trunks;
- accepted work should migrate into an appropriate maintained baseline;
- repository structure should communicate architectural boundaries clearly;
- generic modules must not quietly become harness-specific dumping grounds;
- generated outputs, caches, dependencies, and transient artifacts stay out of source control unless intentionally required;
- secrets never enter repository history;
- stale documentation must be corrected, superseded, or clearly marked historical;
- temporary workarounds do not become permanent architecture merely because they succeeded once;
- technical debt must be explicit rather than silently normalized;
- dependencies must have a justified purpose;
- refactors must preserve previously verified behavior unless change is explicitly intended;
- dead or superseded implementation paths should be removed deliberately;
- evidence and implementation must remain distinguishable.

The standard is:

> **Leave the project cleaner, clearer, and more modular than the state in which the work began whenever the authorized scope permits it.**

---

## 12. Evolution and Change Control

HarneSSHost is intended to evolve.

This doctrine defines enduring constraints and direction, not a frozen technical implementation.

New harnesses, interfaces, runtime environments, provider protocols, evaluation methods, or evidence may justify architectural evolution.

Future work may introduce new abstractions when concrete evidence demonstrates that they are necessary.

Future work must not:

- silently contradict established doctrine;
- erase harness fidelity for convenience;
- collapse independently meaningful variables;
- weaken security or ownership boundaries;
- knowingly degrade repository hygiene;
- convert a temporary implementation detail into permanent product doctrine without review.

A doctrine-level decision may be changed when evidence or product direction warrants it, but the change must be explicit and Director-approved.

Superseded doctrine should remain traceable rather than silently rewritten from project history.

---

## 13. Governing Principle

HarneSSHost should remain:

> **modular enough to support fundamentally different harnesses, faithful enough to preserve what makes each harness distinct, rigorous enough to evaluate them honestly, usable enough to serve users with different technical skill levels, and disciplined enough that the project itself never becomes the complexity it was designed to manage.**
