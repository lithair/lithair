# Security Policy

## Supported versions

Lithair is pre-1.0. Security fixes target the **latest released minor**
only — earlier minors do not receive backports.

| Version  | Supported          |
| -------- | ------------------ |
| 0.12.x   | :white_check_mark: |
| < 0.12   | :x:                |

When a new minor is released, the previous minor moves out of support
immediately. The fix lands in the next minor (e.g. a vuln triaged during
0.12.x ships in 0.13.0, not in a 0.12.y patch, unless the maintainer
decides the severity warrants a same-minor patch release).

## Reporting a vulnerability

**Do not open a public issue for security bugs.**

Report privately via GitHub Security Advisories:

> <https://github.com/lithair/lithair/security/advisories/new>

This opens a private advisory visible only to the maintainer. Include:

- A description of the vulnerability and its impact.
- Reproduction steps (a minimal example or PoC is ideal).
- The Lithair version and Rust toolchain you tested against.
- Any suggested remediation, if you have one.

## Response

- **Triage**: within **7 days** of report. You'll get an acknowledgement
  and an initial severity assessment.
- **Fix**: targeted for the next minor release. Critical issues (auth
  bypass, RCE) may ship as an out-of-band patch on the current minor.
- **Disclosure**: coordinated. The advisory stays private until a fix is
  released; the reporter is consulted on the public disclosure timing.

The maintainer is solo and best-effort. The 7-day triage commitment is
genuine, but post-triage fix timelines depend on severity and complexity.

## What counts as a vulnerability

In scope:

- **Authentication or authorization bypass** in the framework's RBAC,
  session, or JWT code.
- **Remote code execution** through any framework-provided code path.
- **Denial-of-service** in the HTTP server, router, firewall, or cluster
  consensus path (resource exhaustion, panic-on-input, unbounded growth).
- **Data corruption or loss** in the event store, snapshot machinery, or
  Raft replication.
- **Information disclosure** caused by the framework's *defaults* —
  e.g. a default config that exposes internal data, leaks credentials in
  logs, or bypasses field-level RBAC.

Out of scope:

- Misconfigurations introduced by the user (overly permissive RBAC rules,
  disabled validators, custom handlers that bypass framework checks,
  weak secrets, etc.).
- Vulnerabilities in third-party dependencies that are not exploitable
  through Lithair's public API (please report those upstream).
- Issues that require physical access to the host or pre-existing
  privileged access to the deploying machine.
- Theoretical attacks without a demonstrated impact.

## Disclosure and credit

Coordinated disclosure is preferred. Reporters who follow this policy
will be credited in the `CHANGELOG.md` entry for the release that
contains the fix, unless they explicitly request to remain anonymous.

Full public disclosure before a fix is released is strongly discouraged
and will not be coordinated with credit.
