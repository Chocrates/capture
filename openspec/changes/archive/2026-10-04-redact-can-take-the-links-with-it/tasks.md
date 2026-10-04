## 1. The redaction

- [x] 1.1 `Session::redact_all(&[tgt])` names several targets in one `RedactBody`, so one cancellation covers all of them — splitting it would make undoing a mistake a matter of catching each one inside its own window. `redact` now delegates to it.
- [x] 1.2 Over the cap is refused there as well as held by the fold. The fold's refusal protects a replica from a peer; this one tells the person standing here why nothing happened, which the fold cannot.

## 2. The command

- [x] 2.1 `--with-links` gathers the target's backlinks, one hop.
- [x] 2.2 The confirmation lists every note by name, under a line saying they carry the target's name in their own text.
- [x] 2.3 Over the cap refuses and says how many there were.

## 3. Seeing it

- [x] 3.1 Target and backlink both destroyed.
- [x] 3.2 A second-hop note survives — it copied the *middle* note's name, not the target's.
- [x] 3.3 Without the flag only the target goes.
- [x] 3.4 Over the cap destroys nothing and the refusal names the limit.
- [x] 3.5 Driven on a scratch store: the confirmation lists both linking notes before anything is queued.
- [x] 3.6 Non-vacuous, with the sabotage confirmed to have applied — stubbing the backlink lookup leaves the confirmation listing nothing.

## 4. Noticed while testing

- [x] 4.1 The linking notes' own slugs contained the target's words — `vault-passphrase-swordfish-…-p905hrxccp` — because a slug is derived from opening words and the link name is in them. That is §1.1 limit 5 happening in front of you, and it is why the mitigation exists.
- [x] 4.2 A test resolving a linking note by slug *after* the redaction fails, because a destroyed note's slug is re-derived from its id. That half of limit 5 already worked.
- [x] 4.3 `alloc_vec` became dead once `redact` delegated, and clippy caught it.
