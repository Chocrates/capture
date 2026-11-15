## ADDED Requirements

### Requirement: A page edited on two devices keeps both versions

Both versions SHALL be kept when the same page is edited on two devices that
have not seen each other's edit. After sync both devices SHALL show the same
version, and the page SHALL report one divergence until it is resolved.

#### Scenario: Two edits, one sync
- **WHEN** device A edits a page to "plan A" and device B edits it to "plan B" before syncing
- **AND** the devices sync
- **THEN** both devices show the same version
- **AND** both report the page as diverged, with both versions readable

#### Scenario: Resolved on one device
- **WHEN** the divergence is resolved on device A and the devices sync
- **THEN** device B no longer reports it

### Requirement: A tick never lands on a different line

A checklist tick SHALL apply to the item it was made on, or be reported as not
matched. After a concurrent reword and sync it SHALL NOT tick a different item.

#### Scenario: Tick races a reword
- **WHEN** device A ticks "eggs" and device B rewords "eggs" to "free-range eggs", then they sync
- **THEN** either "free-range eggs" is ticked or no item is ticked and the tick is reported unmatched
- **AND** no other item is ticked

### Requirement: Links name what they meant when written

A `[[name]]` link SHALL keep the target it resolved to when written. Changing or
reassigning a page's name SHALL NOT silently retarget existing links; a link
whose name no longer resolves to its target SHALL be shown broken.

#### Scenario: A page is renamed
- **WHEN** a page named `packing` is renamed `packing-list`
- **THEN** existing `[[packing]]` links are shown broken
- **AND** they still record the page they meant

### Requirement: A credential-shaped title gives a page no name

When a new page's title looks like it holds a credential, the page SHALL be
created without a chosen name, SHALL keep its date slug, and the surface SHALL
say why.

#### Scenario: A token in the title
- **WHEN** a page is made with the title "deploy token ghp_a1b2c3d4e5f6g7h8i9j0"
- **THEN** the page has no alias
- **AND** the surface says the title looked like a credential

### Requirement: Every version of a note is checked for credentials

A note SHALL be flagged as holding a credential when any version of its text is,
not only the first.

#### Scenario: A secret pasted into a page
- **WHEN** a page is edited to contain "password: hunter2hunter2hunter2"
- **THEN** the note is flagged
- **AND** it is withheld from export as a flagged capture is

### Requirement: Search reads what is typed as words

Search SHALL treat its input as words to find, never as query syntax. No input
SHALL produce an error. A match in a note's first line SHALL rank above a match
only in its body.

#### Scenario: Punctuation in a search
- **WHEN** the person searches for `"half-open` or `re: deploy`
- **THEN** matching notes are returned, or none
- **AND** no error is shown

#### Scenario: A title outranks a mention
- **WHEN** one page is titled "Car maintenance" and another mentions car maintenance in its body
- **THEN** a search for "car maintenance" lists the titled page first

### Requirement: An edit can refuse to overwrite a newer version

An edit that names the version it was written against SHALL be refused, and
nothing SHALL be written, when the note's current version is a different one.
Naming a version is optional.

#### Scenario: Two writers on one device
- **WHEN** a script reads a page at version v1, the person then saves v2, and the script edits naming v1
- **THEN** the script's edit is refused with a conflict
- **AND** the page is still v2

### Requirement: Redacting a page names what links to it and nests under it

The confirmation for redacting a page SHALL name the notes that link to it and
the pages under it. After redaction their links SHALL show broken and the
children SHALL have no parent.

#### Scenario: A page with a child and a linker
- **WHEN** a page with one child page and one linking note is redacted
- **THEN** the confirmation names both
- **AND** afterwards the link shows broken and the child has no parent
