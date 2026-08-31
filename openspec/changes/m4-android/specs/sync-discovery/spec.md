## ADDED Requirements

### Requirement: Multicast receive requires a held lock on some platforms

Where the host platform requires a lock to receive multicast, discovery SHALL acquire it before
browsing and SHALL report when it cannot.

On Android, `WifiManager.MulticastLock` is that lock, and without it multicast receive produces no
error and no packets. Discovery appears to run and finds nothing, which is indistinguishable from
an empty network — so the absence must be reported rather than inferred from silence.

#### Scenario: The platform requires a lock
- **WHEN** discovery starts on a platform that requires a multicast lock
- **THEN** the lock SHALL be held for as long as browsing continues
- **AND** released when it stops

#### Scenario: The lock is unavailable
- **WHEN** the lock cannot be acquired
- **THEN** discovery SHALL report that it is not running
- **AND** the remembered-address fallback SHALL be used instead
