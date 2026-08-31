## Why

There is one device, and §1.2 names the failure that makes this urgent rather
than merely next: a capture that exists on exactly one machine is one disk away
from not existing. Everything M1 and M2 built — the log, the conflict rules, the
occurrence exceptions, the promotion authority rule — was designed for
concurrent writers and has never met one. The property tests exercise
convergence over synthetic event sets; nothing has ever folded a stream that
arrived over a wire.

§8.10 puts sync here for a second reason: **M4 is Android, and Android without
sync is a second isolated store.** A phone that captures into its own archive is
not a capture surface for this system, it is a different system with the same
name. The value of the phone is that it holds the same archive; that value is
entirely supplied by M3.

The design is unusually complete on this (§5.1–§5.6, §6), and the parts that
normally make sync hard have already been paid for. Single-writer-per-origin
with gap-free `seq` means reconciliation is a version-vector diff and nothing
more — no Merkle tree, no bloom filter, no set-difference estimator. The
complexity lives in the data model, where it has been since M1.

## What Changes

A new crate, `brain-sync`, and the operational surface to pair a second device
and watch it work.

- **Transport.** TCP with the Noise_KK handshake: mutual authentication and
  forward secrecy from keys both sides already hold, no PKI and no CA. Every
  frame after the handshake is a Noise transport message — there is no
  pre-handshake message and no plaintext fallback, because the blob plane moves
  large byte ranges and "just this one over a plain socket" is the accident this
  rule exists to prevent.
- **Discovery.** mDNS/DNS-SD over `_bsp._tcp.local`, with a rotating
  per-network pseudonym and a keyed version-vector digest rather than a stable
  id and a plain digest. A stable identifier broadcast on every network you join
  links your devices to each other at every café and airport, and a plain digest
  is an activity oracle — a passive observer counts your captures without ever
  completing a handshake. Static last-known-address fallback with jittered
  backoff, because mDNS is blocked on many networks.
- **Membership.** The roster lives in the log (§5.3). A device is paired once,
  out of band, with any single existing device; `device_add` is what admits it,
  and every other device learns of it by folding the log rather than by being
  paired again. `brain-core` already has the roster fold and the event kinds —
  what is missing is the pairing gesture and the transport that uses them.
- **Anti-entropy**, which is where correctness lives. `HELLO` exchanges version
  vectors, each side computes its own diff, `RANGE` requests fill it. Runs on
  session open, on network change, on wake, and on a timer. Gossip fanout of
  2–3 rather than a full mesh, so nothing has to be elected and no device has to
  be online for the archive to converge.
- **Live push**, which is a latency optimisation and is allowed to fail
  silently — with one exception. A `redact` is pushed eagerly and retried until
  acknowledged, because for redaction the delivery latency *is* the guarantee
  (§1.1).
- **Sealing**, with the correction §5.5 makes to the obvious version: a sealed
  origin keeps a permanent one-line entry recording its final `seq` and final
  event id. Dropping the entry outright breaks the completeness oracle — a new
  device asks what it is missing, is told nothing, and believes an archive with
  holes is whole. Worse, an unreachable redaction becomes permanently
  bypassable.

## Capabilities

### New Capabilities

- `sync-transport`: Noise_KK sessions, framing, and the rule that nothing
  crosses the wire outside one.
- `sync-discovery`: mDNS advertisement and the rotating pseudonym, plus the
  static fallback.
- `sync-membership`: pairing a second device, the roster in the log, and what a
  device that is not on it can and cannot do.
- `sync-antientropy`: version vectors, `HELLO`/`RANGE`, peer selection, and
  sealing.
- `sync-push`: live push, and redaction's exception to its being best-effort.

### Modified Capabilities

- `event-log`: ingest gains a second source. Events arriving from a peer are
  verified, ordered and folded exactly as local ones are — the requirement
  changing is that "the log is append-only and single-writer per origin" now has
  to hold across a wire, where an origin's stream can arrive out of order, twice,
  or interleaved with its own local appends.
- `cli`: pairing, peer status, and what to say when a peer has not been seen —
  §4.5 puts "a peer not reached in over a week" in tier 1, because it is the
  leading indicator of the failure this milestone exists to prevent.

## Impact

- **New crate**: `brain-sync`. Its first dependencies on a network stack and on
  Noise — the first code in this project that opens a socket.
- **`brain-core`**: no new event kinds are expected. The roster fold, `device_add`
  and `origin_claim` already exist and were built for this.
- **`brain-store`**: ingest from a peer, the version vector read off `heads`, and
  the sealed-origin bookkeeping.
- **`brain-cli`**: `brain pair`, `brain peers`, and the status surface for both.
- **`brain-capture`**: untouched. §8.10 freezes the capture gesture for the
  duration, and the overlay is the elected writer — the sync daemon must not
  take that role from it.
- **Threat model**: this is the first milestone where an attacker on the network
  is in scope. §5.3's threat model is a guest phone and a housemate's laptop, and
  the design's answers — Noise_KK, the rotating pseudonym, the keyed digest —
  are requirements rather than hardening to add later.
- **Testing**: the conflict rules have been property-tested against synthetic
  concurrency since M1. This milestone is where they meet a real second writer,
  and the acceptance bar is that two devices reach the same fold with no
  coordination and no device required to be online.
