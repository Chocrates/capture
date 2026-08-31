## ADDED Requirements

### Requirement: Discovery holds a multicast lock, and says so when it cannot

While browsing for peers the application SHALL hold a `WifiManager.MulticastLock`. Where the lock
cannot be acquired, discovery SHALL report that it is not running.

Without the lock, Android's multicast receive silently does nothing: no error, no callback, no
symptom. It is indistinguishable from there being no peers on the network, which is the most
misleading failure this system could present — the user would conclude sync is broken, or worse,
that it is working and there is nothing to fetch.

#### Scenario: The lock cannot be acquired
- **WHEN** discovery starts and the multicast lock is refused
- **THEN** the condition SHALL be reported
- **AND** the phone SHALL fall back to remembered addresses rather than appearing to search

### Requirement: The phone is intermittently reachable by design

Sync SHALL run under the platform's background work scheduler with network and battery constraints,
and the application SHALL NOT attempt to defeat doze.

The protocol was written assuming devices are off, asleep or absent (§7 failure mode 1). Fighting
the platform to hold a socket open would cost battery to achieve what anti-entropy achieves for
free on the next wake.

#### Scenario: The phone has been asleep
- **WHEN** the phone wakes after a period of doze
- **THEN** it SHALL reconcile with whatever peers are reachable
- **AND** events captured while it slept SHALL propagate

### Requirement: Staleness is shown, never implied

Surfaces SHALL report when a peer was last reached rather than presenting current data as
necessarily fresh.

A phone showing stale data is correct. A phone implying freshness it does not have is not, and the
difference matters most on the device that spends most of its time asleep.

#### Scenario: A peer not reached recently
- **WHEN** a peer has not been reached for some time
- **THEN** the surface SHALL say when it was last reached
- **AND** a peer unreached for over a week SHALL be reported as needing attention
