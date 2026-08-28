## Context

`brain-core` already folds a roster and knows `device_add` and `origin_claim`;
`brain-store` already keeps a gap-free `seq` per origin in `heads`, which is a
version vector under another name. The conflict rules have been property-tested
against synthetic concurrency since M1. What has never happened is two processes
on two machines writing to the same archive.

Nothing in this project has opened a socket before. That is the whole of the new
risk surface, and §5.3's threat model is specific about who is on the other side
of it: not a nation state, but the guest phone on the café wifi and the
housemate's laptop on the same LAN.

The single-writer-per-origin decision from §1 is what makes the protocol small.
An origin's events are a gap-free sequence, so "what am I missing from you" is a
subtraction, and reconciliation needs no Merkle tree, no bloom filter and no
set-difference estimator. This design's job is mostly to avoid squandering that.

## Goals / Non-Goals

**Goals:**

- Two devices converge on the same fold with no coordinator and no device
  required to be online.
- A capture on either device is on the other within seconds when both are up,
  and eventually whenever they next meet.
- A `redact` reaches every reachable peer promptly and is retried until
  acknowledged, because for redaction the latency is the guarantee (§1.1).
- Nothing readable crosses the LAN, and nothing about capture volume or device
  identity leaks to an observer who never completes a handshake.
- Pairing is one gesture, once, with any one existing device.

**Non-Goals:**

- Internet sync, relays, or NAT traversal. LAN and static addresses only.
- The blob plane beyond references. Audio and images are M5's problem; this
  milestone moves events.
- Leaf replicas. §5.5 specifies them carefully and they exist for a tablet or a
  work machine — neither of which exists yet, and a partial replica whose
  coverage nothing tests is a way to lose events quietly.
- Sealing quorum mechanics beyond recording the entry. Sealing needs unanimity
  among roster members (§5.5); with two devices that is not a consensus problem
  worth building machinery for.
- Any change to the capture gesture. §8.10 freezes it, and the overlay stays the
  elected writer (§8.8) — the sync daemon appends through the same lock as
  everything else rather than becoming a second writer.

## Decisions

**A separate crate, `brain-sync`, rather than a module in `brain-store`.**
The purity argument that keeps a clock out of `brain-core` applies here in the
other direction: a socket is the least deterministic thing in the system, and
the boundary between "events arrived" and "events folded" is exactly where a bug
becomes unreproducible. `brain-sync` owns the wire; it hands `brain-store`
verified envelopes and nothing else. Alternative considered: put it in
`brain-store` next to ingest. Rejected because it would make every store test
link a network stack.

**Noise_KK, not TLS.** Both sides already know each other's static public key —
that is what pairing established — so there is no PKI to operate, no certificate
to expire, and no CA. KK gives mutual authentication and forward secrecy from
material already in the log. Alternative: TLS with self-signed certs and pinning.
Rejected: it reimplements pairing badly and adds a certificate lifecycle nobody
wants to own on a device that may be off for a month.

**One rule about the wire, stated as a rule because it is violated by
convenience:** every message is a Noise transport message inside an established
session. No pre-handshake message, no plaintext fallback, no small HTTP endpoint
for blobs. The failure this prevents is specific — audio in the clear on the LAN
while the note describing it is encrypted.

**Frames cap at 65519 usable bytes**, which is the Noise message limit minus the
AEAD tag. Blob chunks are 32 KiB so a chunk fits inside one message with room
for framing. An earlier draft of the design specified 65536-byte chunks, which
do not fit at all; the number is written down here so it cannot drift back.

**Discovery advertises a rotating pseudonym and a keyed digest.** Both are
derived from the roster key, so members recognise each other and nobody else can
correlate a device across networks or count captures from the outside. This is
not hardening deferred to later: a stable id broadcast in the clear on every
network is precisely the cross-network identifier §5.3 refuses, and it is
cheaper to do right first than to change once devices are advertising.

**Anti-entropy is the correctness mechanism; live push is an optimisation.**
Push may fail silently and must never be the only path an event takes, because
anything that relies on delivery having happened is a bug that appears only when
the network is bad. `redact` is the exception, and it is an exception in
*urgency* rather than in *correctness*: it is retried until acknowledged, and
anti-entropy would still deliver it eventually.

**Sealed origins keep a permanent entry.** §5.5 corrects the obvious version at
length and the correction is load-bearing: a sealed origin whose entry vanishes
takes the completeness oracle with it, so a newly paired device receives none of
that origin's events and reports agreement with every peer. ~40 bytes per
retired origin forever is the honest cost.

**Ingest goes through the existing writer lock.** A peer's events are appended by
the same path as local ones, which means verification, `bhash` checking and the
quarantine rules already written in M1 apply unchanged. Alternative: a separate
ingest path optimised for batches. Rejected — two ways into the log is two sets
of rules about what may enter it.

## Risks / Trade-offs

- **A socket is a new class of bug in a system that has been deterministic.** →
  `brain-sync` hands over verified envelopes and owns nothing else; everything
  after that point is the fold, which is testable without a network. Protocol
  logic is written against an in-memory transport so the tests do not need one
  either.
- **Two writers will find bugs the property tests did not.** → That is the point
  of the milestone rather than a risk to avoid, but it argues for the acceptance
  criteria being about *observed convergence between two real devices*, not
  about the protocol matching its own description.
- **mDNS is blocked on many networks, and the fallback is a cache of addresses
  that go stale.** → Jittered backoff and an honest status line. A peer not
  reached in over a week is a tier-1 health condition (§4.5) precisely because
  silent non-sync is the failure mode that loses data.
- **Pairing is the one moment a wrong decision admits a device permanently.** →
  Out of band, one gesture, and `device_add` is an ordinary event, so an
  unexpected one is visible in the health surface (§4.5 makes it tier 1 and
  non-dismissible until acknowledged).
- **A second device makes redaction genuinely harder.** → §5.4 and §1.1 already
  say what to do: push eagerly, retry until acknowledged, and report a redaction
  outstanding against an unreachable peer rather than pretending it completed.
- **Scope creep toward the blob plane.** → Explicitly out. Events only, and the
  chunking decision is recorded so the blob work has somewhere to start.

## Migration Plan

There is nothing to migrate. A single-device store is a valid archive and stays
one; sync adds a second origin rather than changing what the first wrote. A
device that has never paired behaves exactly as it does today, and the sync
daemon is not started unless a roster has more than one member.

Rollback is stopping the daemon. Events already received are ordinary events and
remain folded — there is no state that only makes sense while sync is running.

## Open Questions

- **Where does the sync daemon live?** The overlay is already resident and is
  the elected writer, which argues for putting the loop there rather than adding
  a second always-on process. Against: an overlay that must never block the
  capture gesture is a poor host for a network loop. Leaning toward a separate
  process that appends through the same lock.
- **What does the first pairing gesture actually look like?** The design says
  "out of band" and does not choose. A short code typed on both sides is the
  cheapest thing that resists a LAN attacker; a QR code needs a camera, which
  the desktop does not have.
- **How much of §5.5's peer selection is worth building at n = 2?** Gossip
  fanout degenerates to a full mesh at two or three devices. The risk of writing
  it later is that "sync with the one other device" quietly becomes an
  assumption; the risk of writing it now is machinery with no second case to
  test it against.
