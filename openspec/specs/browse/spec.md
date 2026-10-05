# browse Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The note page shows the structured half beside the text

The note page SHALL show the captured text verbatim together with the current
fields and the regenerated descriptor, so that drift between what was captured
and what the note now is remains visible.

This SHALL hold on every surface that has a note page, not only the desktop. A
phone that showed the text alone would be a second answer to "what is this
note", and the one most people actually read.

#### Scenario: Both halves are on screen
- **WHEN** a promoted note is opened
- **THEN** the captured text and the current fields are both shown

#### Scenario: Both halves are on screen, on every surface
- **WHEN** a promoted note is opened on the phone
- **THEN** the captured text and the current fields are both shown

#### Scenario: A suggestion below the floor is offered, not applied
- **WHEN** a note carries a recognition below the confidence floor
- **THEN** it is shown as a single line offering the interpretation
- **AND** nothing has been applied to the note

### Requirement: The calendar renders all-day events apart from the timed grid

All-day events SHALL render in a separate band above the timed grid, because
they are floating dates with no position on an hour axis. The timed grid SHALL
draw expanded occurrences with overlap resolved by column packing.

#### Scenario: A vacation is not placed on the hour axis
- **WHEN** an all-day event is displayed
- **THEN** it appears in the all-day band

#### Scenario: A DST-shifted occurrence is marked
- **WHEN** an occurrence was shifted by a DST transition
- **THEN** the calendar marks it rather than showing it silently at the shifted time

#### Scenario: A cancelled occurrence is hidden but not gone
- **WHEN** an occurrence has been cancelled
- **THEN** it is not drawn
- **AND** it remains recoverable because the cancellation is an ordinary event

### Requirement: The agenda is a view of the same substrate

The agenda SHALL present what is due and what occurs, drawn from the same notes
and occurrences as every other view, and SHALL NOT be a separate species of
object.

#### Scenario: A meeting and its notes are one record
- **WHEN** a meeting is opened from the agenda
- **THEN** the notes taken during it are on the same page

### Requirement: Every mutation is reachable from the window

Every first-party surface SHALL offer every mutation the command line offers
against a note, and SHALL offer device management where the device is already
shown.

§8.4 makes the window the first-party surface, and §8.10 scoped it to "every
mutation" for exactly this reason: muting exists so that suppressing something
is *cheaper* than destroying it, and that gradient inverts the moment
suppression needs a shell while destruction is a button. The same argument
applies to a lost device — the moment somebody needs to retire one is not a
moment to be looking up a command.

The phone is held to the same rule. It is a first-party surface by the same
argument, and the gradient it inverts is sharper: it is where captures are made
and so where a wrong kind is first seen.

Splitting a series SHALL be offered only on a series, since it is the only
thing a rule can be split at.

#### Scenario: Changing a series from the window
- **WHEN** a note is a series
- **THEN** the page SHALL offer to change it from a date onward
- **AND** SHALL NOT offer it on a note that is not a series

#### Scenario: Retiring a device from the window
- **WHEN** a device other than this one is listed
- **THEN** it SHALL be possible to retire it
- **AND** this SHALL require the device's key to be typed, as destruction
  requires the note's name

#### Scenario: Adding a device from the window
- **WHEN** the user starts a pairing
- **THEN** the code SHALL be shown immediately
- **AND** the window SHALL remain usable while it waits, because the answer
  arrives whenever somebody walks to the other device

#### Scenario: Changing a kind from the phone
- **WHEN** a capture's kind is shown on the phone
- **THEN** changing it SHALL be reachable without a command line

### Requirement: Recovering from a lockout stays a command

Re-admission after every device has been retired SHALL NOT require a window
surface, and its absence there is deliberate rather than unfinished.

§8.4's argument is about *ordinary* gestures: muting must not cost more than
destroying. Re-admission is neither ordinary nor a gesture — it is two long
signatures carried between two machines, in a state where nothing else works —
and a form for pasting hex is not what makes that recoverable. What makes it
recoverable is that it needs no server and no network.

#### Scenario: A lockout
- **WHEN** every device has been retired
- **THEN** recovery SHALL be possible from the command line alone
- **AND** SHALL require two devices, because one compromised device is what
  caused the lockout

### Requirement: A shared confirmation says what it is confirming

Where one dialog serves more than one irreversible action, it SHALL state the
action it is confirming rather than the one it was written for.

This is not presentation. The dialog was written for destruction and says so in
the markup; reused unchanged it asked "Destroy this content?" over a device
retirement, which deletes nothing. Confirmation is the one screen whose entire
job is being precise about what is about to happen.

#### Scenario: Confirming something that is not a destruction
- **WHEN** the confirmation is raised for an action that destroys nothing
- **THEN** its title, explanation and accept button SHALL describe that action

### Requirement: A row renders what the capture is, not only what was typed

A row in a reading surface SHALL render the promotion the capture carries — a
task with its state, an event with its time, a series with its rule, a list
with its items — and SHALL show the captured text alongside it.

A capture carrying no promotion SHALL render as its text alone. A note's
content is its text, and a descriptor invented for one would add a fact the
capture did not contain (§3.1.3).

The captured text SHALL NOT be replaced by the rendering. §3.1.4 requires the
text, the fields and the descriptor to be visible together so that a
disagreement between them can be seen; a row showing only the fields would hide
the disagreement the descriptor exists to reveal.

#### Scenario: A task reads as a task
- **WHEN** a capture promoted to a task with a due date appears in a timeline
- **THEN** the row shows that it is a task and when it is due
- **AND** the captured text is still shown

#### Scenario: An ordinary note is unchanged
- **WHEN** a capture carrying no promotion appears in a timeline
- **THEN** the row shows its text
- **AND** no descriptor is invented for it

#### Scenario: Drift stays visible
- **WHEN** a capture reading "every day at 7" carries fields saying weekdays
- **THEN** both the text and the rendered rule are on the row

### Requirement: A list renders as a list wherever it appears

A capture recognised as a list SHALL render as its items, each showing whether
it has been completed, and each completable in place. This SHALL NOT require
navigating to a separate surface first.

A tag-wide list view gathers items across several captures and remains
available; it is not the only way to reach the items of one capture.

#### Scenario: A list capture is a list on sight
- **WHEN** a list capture appears in a timeline
- **THEN** its items are shown with their completion state

#### Scenario: An item is completed where it is drawn
- **WHEN** an item is completed from the row it appears on
- **THEN** it is marked done
- **AND** no navigation was required to do it

#### Scenario: The sigil is not what the reader is left with
- **WHEN** a capture reading `#groceries: milk, eggs, bread` is drawn
- **THEN** the reader is shown three items

### Requirement: People and projects are browsable, on every surface

Every first-party surface SHALL offer an index of the people and projects the
archive knows, each reachable as an entity page, and SHALL show how many notes
name each.

The phone is included, and its absence there today is the gap this closes. §8.4
makes reachability the rule and §3.7 makes entities one of four axes that
populate without the user — an axis that populates by itself and is then only
readable on a laptop is one that does not pay for itself on the device where
capture happens.

An index entry SHALL open a page of its own rather than a note, because an entity
is not a note. Where an entity has a backing note, the page SHALL show it above
the timeline.

#### Scenario: A person opens their own page
- **WHEN** a person is chosen from the index
- **THEN** an entity page SHALL open
- **AND** it SHALL NOT open a note instead

#### Scenario: The phone can browse people
- **WHEN** the archive holds notes naming people
- **THEN** the phone SHALL offer them as an index
- **AND** each SHALL lead to the notes naming that person

#### Scenario: Nothing named yet
- **WHEN** no note names anybody
- **THEN** the index SHALL say so plainly
- **AND** SHALL NOT present itself as broken or unconfigured

### Requirement: A mention in a note is a way to the page

Where a surface renders note text, a mention in it SHALL be a way to that
entity's page.

A mention is already the link §4.3 calls the one that carries the load — it
exists without anyone having linked anything. Rendering it as inert text on one
surface and as a link on another makes the same note mean different things
depending on where it is read.

#### Scenario: Following a mention
- **WHEN** note text containing `@sarah` is shown
- **THEN** that mention SHALL lead to her entity page

#### Scenario: A mention that was renamed still leads somewhere
- **WHEN** the entity a mention names has been renamed
- **THEN** the mention SHALL still lead to that entity's page

### Requirement: Renaming, merging and binding are reachable where the page is

An entity page SHALL offer to rename the entity, to merge it into another, and to
bind or unbind a backing note.

These exist because extraction cannot get identity right on its own, and a
correction that needs a different machine than the one that found the problem is
the gradient §8.4 refuses. The phone is where captures are made, so it is where a
duplicated person is first noticed.

Merging SHALL name what it will join and SHALL be confirmed, because it is the
one operation here that makes two pages into one.

#### Scenario: Renaming from the page
- **WHEN** an entity is renamed from its page
- **THEN** the new name SHALL be displayed
- **AND** its notes SHALL be unchanged

#### Scenario: Merging says what it is joining
- **WHEN** a merge is offered
- **THEN** it SHALL name both entities before it is carried out

#### Scenario: Binding a backing note
- **WHEN** a note is bound as the backing note
- **THEN** it SHALL show above the timeline on that page
- **AND** unbinding SHALL be available on the same terms

### Requirement: A capture's checklist is reachable from the capture

Where a surface shows a capture that wrote items, it SHALL offer that capture's
own checklist, and ticking from there SHALL be the same write as ticking
anywhere else.

A collection under a tag SHALL remain available and SHALL be presented as what
it is: everything carrying that tag, from however many captures.

#### Scenario: Ticking from the capture
- **WHEN** a capture's own items are shown
- **THEN** each SHALL be tickable
- **AND** the result SHALL be identical to ticking it from the tag's collection

#### Scenario: A collection says how many captures it gathers
- **WHEN** a tag's collection holds items from more than one capture
- **THEN** the surface SHALL make clear they are not one capture's list

### Requirement: A near miss is offered where a suggestion is offered

A surface SHALL show a near miss wherever it shows a recognition it was not
confident enough to act on, and SHALL allow it to be accepted.

Accepting SHALL write an ordinary edit that inserts what was missing. The system
SHALL NOT make that edit on its own: captured text is kept verbatim (§3.1.3),
and a correction to it is a thing a person does.

The offer SHALL say what will change, because it rewrites the text of a capture
rather than adding a promotion beside it — which is a different kind of act from
accepting a type, and the surface should not make them look the same.

#### Scenario: Accepting a missing colon
- **WHEN** a capture missing its colon is shown
- **THEN** the offer SHALL name what will be inserted
- **AND** accepting SHALL make the capture a list

#### Scenario: Declining changes nothing
- **WHEN** a near miss is offered and not accepted
- **THEN** the capture SHALL be unchanged
- **AND** the offer SHALL still be available later

#### Scenario: Once it is a list there is nothing to offer
- **WHEN** a near miss has been accepted
- **THEN** no near miss SHALL be offered for that capture

### Requirement: A task surface answers what is outstanding

Every first-party surface SHALL offer a view of tasks, banded into overdue, due
on or before a horizon, and open with no due date.

A task has state and occupies no time (§3.6), so the agenda cannot hold it: the
agenda is a view of time and a task has none. Without a surface of its own an
undated task is reachable only by scrolling the timeline, which is ordered by
when a thing was captured rather than by whether it is still owed.

The undated band SHALL be present rather than folded into the others. A task
with no due date is the ordinary case — the recognizer refuses to promote a
bare task verb, so most tasks arrive by a person saying so, and a person saying
"send sarah the migration doc" is not saying when.

Done tasks SHALL NOT occupy the bands. Nothing here scores completion or counts
a streak; a finished task simply stops being outstanding.

#### Scenario: An undated task is reachable
- **WHEN** a task carries no due date
- **THEN** it SHALL appear in the task surface's undated band
- **AND** SHALL NOT appear in the agenda

#### Scenario: An overdue task is separated from one merely due
- **WHEN** a task's due date is before today in the reading zone
- **THEN** it SHALL appear in the overdue band
- **AND** a task due today or later SHALL NOT

#### Scenario: Completing a task from the surface
- **WHEN** a task is marked done from the task surface
- **THEN** it SHALL leave the bands
- **AND** the change SHALL be the same `promote` write the command line makes

#### Scenario: Nothing outstanding
- **WHEN** no task is open
- **THEN** the surface SHALL say so plainly
- **AND** SHALL NOT present an empty list as an error or a failure

### Requirement: A wrong kind is correctable from the surface that shows it

Where a surface renders what a capture is, it SHALL offer to change it. A
correction SHALL be pinned, and so SHALL beat every later reparse.

The recognizer deliberately refuses a promotion it is not confident of, and
`browse` already requires that a below-floor recognition be offered rather than
applied. An offer that cannot be accepted where it is read is not an offer. The
phone sees a misparse first because the phone is where capture happens, so the
phone is the worst possible place to be told to go and find a command line.

Demoting to a plain note SHALL be available on the same terms, because a thing
wrongly promoted is as common as a thing wrongly left alone, and `Promoted::Note`
is a promotion like any other.

#### Scenario: Accepting a below-the-floor suggestion
- **WHEN** a note shows a suggested interpretation it is not confident of
- **THEN** it SHALL be possible to accept it from that surface
- **AND** accepting SHALL write a pinned correction

#### Scenario: A pinned correction survives a later sweep
- **WHEN** a kind has been corrected by a person
- **AND** the recognizer later re-parses that capture under a newer grammar
- **THEN** the corrected kind SHALL stand

#### Scenario: Saying a note is a task
- **WHEN** a plain note is named as a task
- **THEN** it SHALL become one, with or without a due date
- **AND** the captured text SHALL be unchanged

#### Scenario: Demoting to a note
- **WHEN** a promoted capture is said to be neither a task nor an event
- **THEN** it SHALL become a plain note
- **AND** the captured text SHALL be unchanged

### Requirement: The window reports sync that has never run

Where the window reports peers, it SHALL first report whether a sync round has
ever run on this device, by the same rule and the same wording the other
surfaces use. A person reading the window is the one least likely to know
whether a daemon is running, and most likely to read "never reached" as a
statement about the network.

#### Scenario: The window says the daemon has not run
- **WHEN** a paired device has never completed a sync round
- **AND** the health surface is shown
- **THEN** it reports that sync has never run on this device

#### Scenario: Every surface agrees
- **WHEN** the same archive is shown in the window, on the phone and on the command line
- **THEN** all three report the same state of sync

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

### Requirement: A list is readable and completable in the window

The window SHALL offer the items written under a tag, gathered from every
capture that wrote them, and SHALL allow each to be completed in place.

A collection is the one surface whose purpose is working through something, and
it was the one surface the window did not have. The note page draws a single
capture's items, which answers a different question: a grocery list is spread
over however many times somebody thought of something.

Completion SHALL be by position within the capture the item came from, never by
its words. Two items reading the same are ordinary on a shopping list and their
words cannot tell them apart.

A read of a list SHALL therefore answer which capture each line came from. A
shape that omits it cannot support completion at all, whatever it draws.

#### Scenario: The items under a tag are shown
- **WHEN** a tag has items written under it by more than one capture
- **THEN** the window SHALL show all of them under that tag

#### Scenario: An item is completed from the list
- **WHEN** an item is ticked in the list view
- **THEN** it SHALL be recorded complete against the capture that wrote it
- **AND** the list SHALL show it complete

#### Scenario: Two items reading the same are told apart
- **WHEN** two captures under one tag wrote the same words
- **THEN** ticking one SHALL leave the other untouched

#### Scenario: A collection says it is one
- **WHEN** a list gathers items from more than one capture
- **THEN** the window SHALL say so rather than presenting them as one list

### Requirement: A window command that nothing can reach is not shipped

Every command the window registers SHALL be reachable from the window, and every
command the window calls SHALL exist.

This is §8.4 applied to the boundary rather than to the feature: a command
registered and never called is a capability the window does not have, and a
command called and never registered fails only when somebody presses the
control — reported to them as the store refusing rather than as a fault in the
page.

The check SHALL be mechanical. Both sides are enumerable — a command is an
annotated function, it reaches the page only if the handler list names it, and
the page calls it by a literal — so this needs no call graph and SHALL NOT
depend on one. A command name the page computes rather than writes SHALL be a
failure, because it makes the check blind.

#### Scenario: A registered command nothing calls
- **WHEN** the window registers a command the page never calls
- **THEN** the check SHALL fail, naming it

#### Scenario: A command the page calls that does not exist
- **WHEN** the page calls a command name that is not registered
- **THEN** the check SHALL fail, naming it

#### Scenario: A computed command name
- **WHEN** the page builds a command name instead of writing it
- **THEN** the check SHALL fail, because it can no longer see what is called

### Requirement: The window offers the task surface

The window SHALL offer a view of outstanding tasks in the three bands, and SHALL
allow a task to be completed from it.

The requirement that every first-party surface offers one was written against
all of them, and the window was the one without. The consequence is not a
missing convenience: a task occupies no time, so the agenda cannot hold it, and
the agenda's due band requires a due date — so an **undated** task, which is the
ordinary kind, was reachable in the window only by scrolling a timeline ordered
by when things were captured.

Completing a task from this surface SHALL be the same write the other surfaces
make. Two paths to one state is how two surfaces come to disagree about it.

#### Scenario: The window shows the three bands
- **WHEN** tasks are outstanding in more than one band
- **THEN** the window SHALL show each band, labelled, with the undated band present

#### Scenario: An undated task is reachable in the window
- **WHEN** a task carries no due date
- **THEN** it SHALL appear in the window's undated band

#### Scenario: Completing from the window
- **WHEN** a task is marked done in the window
- **THEN** it SHALL leave the bands
- **AND** the write SHALL be the one the command line makes

#### Scenario: Nothing outstanding reads as nothing, not as a fault
- **WHEN** no task is open
- **THEN** the window SHALL say so plainly

### Requirement: The window corrects a kind where it shows one

The window SHALL offer to make a capture a task, and to demote a promoted
capture to a plain note, from the surface that renders what it is.

The requirement that a wrong kind is correctable where it is read was written
against every surface, and the window had the half that comes from the
recognizer — accepting a suggestion below the floor — and neither of the halves
that come from a person. Saying "this is a task" is the ordinary correction,
because a bare task verb is below the floor by design, so most tasks arrive by
somebody saying so.

A correction SHALL be pinned, and the captured text SHALL be unchanged.

Naming a task and dating it SHALL remain separate: the due date SHALL be
optional, because deciding something is owed and deciding when it is owed are
different decisions and one often arrives without the other.

#### Scenario: Saying a note is a task, from the window
- **WHEN** a plain note is made a task in the window
- **THEN** it SHALL become one
- **AND** the captured text SHALL be unchanged

#### Scenario: With a due date, and without
- **WHEN** a note is made a task and no due date is given
- **THEN** it SHALL be an undated task
- **AND** giving one SHALL set it

#### Scenario: Demoting from the window
- **WHEN** a promoted capture is made a plain note in the window
- **THEN** it SHALL be a plain note
- **AND** the captured text SHALL be unchanged

#### Scenario: The correction outlives a reparse
- **WHEN** a kind has been corrected in the window
- **AND** the capture is later read again under a newer grammar
- **THEN** the corrected kind SHALL stand

### Requirement: A day means the day the reader is in

Where a surface shows what happens on a day, it SHALL take that day to begin and
end in the reading zone, and every surface SHALL agree about which day a given
moment falls on.

A day is not a fixed span of instants. "2026-10-05" begins at a different moment
in Denver than in Berlin, and a surface that builds its window in a zone the
reader is not in answers a question nobody asked — quietly, and worst in the
evening, which is when somebody is most likely to be looking at an agenda.

Two surfaces reading one store SHALL NOT disagree about what day something is
on. That disagreement is worse than either answer alone: it makes the archive
look unreliable rather than the surface look wrong.

The same zone SHALL decide whether the day being shown is today, because that is
what governs whether work already overdue is shown at all, and the overdue
boundary is defined against the reading zone.

#### Scenario: An evening appointment is on the evening's day
- **WHEN** an appointment is at 19:00 in the reading zone, west of UTC
- **THEN** it SHALL appear on that day's agenda
- **AND** SHALL NOT appear on the next day's

#### Scenario: An early appointment east of UTC is on its own day
- **WHEN** an appointment is early in the morning in a zone ahead of UTC
- **THEN** it SHALL appear on that day's agenda
- **AND** SHALL NOT appear on the previous day's

#### Scenario: Two surfaces agree
- **WHEN** the same day is asked for in the window and in the terminal
- **THEN** both SHALL answer with the same entries

#### Scenario: Today is today in the reading zone
- **WHEN** the day being shown is today in the reading zone
- **THEN** work already overdue SHALL be shown with it

