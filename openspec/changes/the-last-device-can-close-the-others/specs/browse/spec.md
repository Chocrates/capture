## ADDED Requirements

### Requirement: A device can be retired from the phone

The phone SHALL be able to retire another device's key. §5.3's remedy for a
lost device is only half reachable while it can be issued from the desktop and
not from the phone: a lost desktop leaves the phone holding the archive, the
lost key still a member, and no surface able to close it.

It SHALL sit where devices are already listed, which is where somebody goes
when something is wrong.

#### Scenario: Retiring a device that was lost
- **WHEN** a device other than this one is retired from the phone
- **THEN** its key is no longer a member
- **AND** the device list reflects that

#### Scenario: The archive decides what retirement means
- **WHEN** the phone retires a device
- **THEN** the key match, the refusal of an ambiguous one, and the event are the archive's

### Requirement: Retiring asks first, and names the device

Retirement SHALL be confirmed before it is carried out, and the confirmation
SHALL name the device being retired. §5.3 makes it recoverable only by a
readmission signed by two devices — and somebody reaching for this has just
lost one, so a mis-tap is close to unrecoverable.

This device SHALL NOT be offered for retirement. Revoking the key of the only
member left is the one state a readmission cannot be written from.

#### Scenario: A confirmation names what will be closed
- **WHEN** retiring is begun for a device
- **THEN** the confirmation names that device
- **AND** nothing is retired until it is confirmed

#### Scenario: Declining changes nothing
- **WHEN** the confirmation is declined
- **THEN** the device is still a member

#### Scenario: This device is not offered
- **WHEN** the device list is shown
- **THEN** no control offers to retire the device being held
