## 1. The crate, and the boundary around the socket

- [x] 1.1 Create `crates/brain-sync`. It owns the wire and nothing else: it hands `brain-store` verified envelopes and never folds, never decides, never touches the projection. The boundary is where an unreproducible bug becomes a reproducible one.
- [x] 1.2 Define every message — `HELLO`, `RANGE`, `EVENT`, `REDACT`, `REDACT_ACK`, `WANT_TGT` — and their encoding, with the same frozen-encoding discipline as `brain-core`: golden vectors before anything writes one. A protocol message is as permanent as an event body once two versions exist in the world.
- [x] 1.3 Write the protocol against a transport trait, with an in-memory implementation. **Every protocol test must run without a socket.** A test suite that needs the network is one that gets skipped, and this is the milestone where skipped tests cost the most. **Done**: `Pair` is two queues and no operating system, and it is a real implementation of the trait rather than a mock — the protocol cannot tell it from a socket. The trait deliberately cannot express a pre-handshake message: a `Transport` is a channel that already exists and is already authenticated, so §5.2's absolute is a thing this crate has no way to violate rather than a rule it remembers to follow.

## 2. Transport — Noise_KK

- [x] 2.1 Implement the handshake using the static keys already in the roster. No PKI, no certificates, no CA to operate on a device that may be off for a month.
- [x] 2.2 Implement framing at 65519 usable bytes, and rekeying on the standard interval so a long transfer cannot exhaust nonces. **Done**: each direction is rekeyed by the count of messages in *that* direction — sender on send, receiver on recv. Driving both from a side's own send count looked symmetric and was not: a device catching up after a week off sends thousands and receives none, so the quiet side would never rotate its incoming key and the transfer would die at the boundary. Tested by a one-way transfer past it, which fails on the earlier version.
- [x] 2.3 **Test that a non-member gets nothing**: no handshake, no message accepted, no change to the archive, and nothing learned beyond connection metadata.
- [x] 2.4 **Test the absolute**: no message of any type is sent or accepted outside an established session, and a failed handshake closes the connection rather than falling back. This is the rule violated by convenience, so it is tested rather than trusted.

## 3. Discovery

- [ ] 3.1 Advertise and browse `_bsp._tcp.local`, with the TXT record carrying the rotating pseudonym and the keyed digest.
- [ ] 3.2 Derive both from the roster key. **Test that an observer who is not a member cannot correlate a device across two networks, and cannot infer capture counts from the digest** — those are the two leaks §5.3 names, and both are cheap now and expensive once devices are advertising.
- [ ] 3.3 Implement the static fallback: last-known addresses, jittered exponential backoff. mDNS is blocked on many corporate and guest networks and a system that only works at home is not a system.
- [ ] 3.4 Skip the connection where the advertised digest says there is nothing to do.

## 4. Membership and pairing

- [ ] 4.1 Choose and implement the out-of-band pairing gesture. The design does not choose one; the constraint is that proximity on the LAN must not be sufficient, and the desktop has no camera. A short code shown on one and typed on the other is the cheapest thing that meets both.
- [ ] 4.2 Append `device_add` on pairing, and let every other device learn the new member by folding. **Test that a third device paired only with the laptop is admitted by the desktop after the desktop syncs with the laptop** — that is the O(n) property the roster-in-the-log exists for.
- [ ] 4.3 Report a `device_add` for a key this device neither paired nor emitted as tier 1, acknowledgeable. `ops.db` already records local pairings, which is what makes "unexpected" computable rather than a guess.

## 5. Anti-entropy

- [ ] 5.1 Read the version vector off `heads` — it is already there, gap-free, and has been since M1.
- [ ] 5.2 Implement `HELLO`/`RANGE` with each side computing its own diff. No negotiation round.
- [ ] 5.3 Run it on session open, on network change, on wake, and on a timer while connected.
- [ ] 5.4 Implement relaying: any device relays any origin's events. **Test that the desktop receives the phone's events while the desktop was off, via the laptop** — "no device is required to be online" is the claim that makes this serverless rather than one machine quietly being the server.
- [ ] 5.5 Peer selection with fanout 2–3. At two devices this degenerates to a full mesh; write it anyway, because "sync with the other one" quietly becoming an assumption is a rewrite later.

## 6. Sealing

- [ ] 6.1 Retain a permanent entry per sealed origin: final `seq` and final event id. **Test the completeness oracle directly** — pair a device into an archive with a sealed origin and assert it can discover the missing events rather than reporting agreement. The bug this prevents is silent and only appears after two reinstalls.
- [ ] 6.2 Name the roster a seal was computed against, and honour a seal only where that roster is agreed current.
- [ ] 6.3 Record the honest cost in the docs: ~40 bytes per retired origin, permanently. The earlier claim that the vector collapses was wrong and the correction is cheaper than the bug.

## 7. Live push, and redaction's exception

- [ ] 7.1 Push new events to connected peers. **Test that a failed push is not an error and changes nothing** — anything that depends on delivery having happened is a bug that only appears when the network is bad.
- [ ] 7.2 Push `redact` eagerly and retry until acknowledged. For redaction the latency is the guarantee (§1.1), and the window between ordering a destruction and its taking effect is the exposure the feature exists to close.
- [ ] 7.3 Report a redaction outstanding against an unacknowledged peer, and keep reporting it for a retired one rather than reporting completion. §5.4 is deliberate that a retired peer stays listed as never acknowledged, because that is the true statement.

## 8. Ingest

- [ ] 8.1 Ingest peer events through the existing append path: signatures, `bhash`, chain, sequence, quarantine. One door into the log.
- [ ] 8.2 Hold an event that arrives ahead of its predecessor rather than folding a stream with a hole or rejecting it. **Test out-of-order and duplicate arrival**, which is what a wire produces and a local append never does.
- [ ] 8.3 **Test that relaying confers no authority**: an event signed by a key that is not its claimed origin is rejected however it arrived, and reported.

## 9. Surfaces

- [ ] 9.1 `brain pair`, `brain peers` — with `--json`, like every other read.
- [ ] 9.2 Report a peer not reached in over a week as tier 1, saying *when* it was last reached rather than only that it is stale. §4.5 promotes this deliberately: a phone that has silently stopped syncing holds captures that exist on exactly one device.
- [ ] 9.3 Show peers and their state in the desktop health surface.

## 10. Where the daemon lives

- [ ] 10.1 Decide and record: a separate process, or the resident overlay. The overlay is already the elected writer, which argues for one process; a loop that must never block the capture gesture argues against. Whatever is chosen, **the capture gesture's latency histogram must be unchanged**, and that is measured rather than asserted.
- [ ] 10.2 Append through the same writer lock as everything else. The sync daemon must not become a second writer — §8.8 has one, and this milestone is not the place to change that.

## 11. Acceptance

- [ ] 11.1 Two real devices, two real captures, one archive. Assert the folds are identical, and assert it after a partition rather than only on a good network.
- [ ] 11.2 Concurrent edits to the same note from both devices, resolved by the rules M1 and M2 property-tested and never once exercised by a second writer. **This is the acceptance test that matters**: everything else confirms the protocol works, and this confirms the data model does.
- [ ] 11.3 A redaction ordered on one device while the other is asleep, arriving and taking effect when it wakes.
- [ ] 11.4 Confirm the capture gesture is unchanged — chord, overlay, commit key, and the latency histogram (§8.10 freezes it for the duration).
- [ ] 11.5 `make ci` passes, including `deny` over whatever a network stack and a Noise implementation drag in. This is the largest new dependency surface the project has taken on.
