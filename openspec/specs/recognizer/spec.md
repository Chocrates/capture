# recognizer Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The three tiers are attempted in order and sigils always win

Recognition SHALL attempt tier 1 (sigils), then tier 2 (the grammar), and SHALL
leave anything both decline to tier 3 or to nothing. Where an explicit sigil and
an inferred expression disagree, the sigil SHALL win and the inference SHALL NOT
be recorded, not even as a suggestion.

#### Scenario: An explicit marker beats inferred text
- **WHEN** a capture reads `>fri` in a line that also says "thursday"
- **THEN** the promotion carries Friday
- **AND** no suggestion of Thursday is recorded

#### Scenario: Tier 2 declining is not an error
- **WHEN** the grammar recognises nothing in a capture
- **THEN** the capture remains an ordinary note
- **AND** nothing is reported as having failed

### Requirement: The recognised vocabulary is closed

Tier 2 SHALL recognise only the words enumerated in `DESIGN.md` §3.1.5. A word
outside that list SHALL NOT be matched approximately, and adding one SHALL be a
deliberate edit rather than a runtime inference.

#### Scenario: An unlisted word is not a near miss
- **WHEN** a capture contains a temporal word absent from the vocabulary
- **THEN** tier 2 does not match it
- **AND** the capture is left to tier 3 or remains a note

### Requirement: Guards prevent matches inside ordinary prose

Recognition SHALL apply the guards of `DESIGN.md` §3.1.5 regardless of what the
grammar alone would accept: an article before a month, weekday or bare clock
prevents a match; a bare number is never a time without a colon, a meridiem or a
marker; a following noun prevents a bare weekday or month matching; and a match
must begin at the head of the capture, at a comma, or after a marker.

#### Scenario: A month inside a noun phrase is not a date
- **WHEN** a capture reads "the march timeline slipped"
- **THEN** nothing is promoted
- **AND** no date suggestion is recorded

#### Scenario: A bare number is not a time
- **WHEN** a capture reads "budget 40k, revisit in Q2"
- **THEN** no time of 40:00 is produced
- **AND** the Q2 marker is recognised on its own terms

#### Scenario: A time behind an article is prose
- **WHEN** a capture reads "the 7am problem"
- **THEN** nothing is promoted

### Requirement: Confidence decides promotion, and the floor is 0.75

Every recognition SHALL carry a confidence score. At or above 0.75 it SHALL
promote. Below it SHALL be recorded as a suggestion surfaced as one line in
review, and SHALL NOT promote.

#### Scenario: An unambiguous datetime promotes
- **WHEN** a capture reads "dentist thursday 2pm"
- **THEN** it is promoted to a timed event

#### Scenario: A bare weekday only suggests
- **WHEN** a capture reads "dentist thursday"
- **THEN** nothing is promoted
- **AND** a suggestion is available in review

#### Scenario: A task verb alone only suggests
- **WHEN** a capture reads "send sarah the migration doc" with no temporal expression
- **THEN** nothing is promoted
- **AND** a suggestion that this may be a task is available in review

### Requirement: Recognition never delays or blocks capture

Parsing SHALL happen after the capture is committed. The overlay SHALL close on
Enter regardless of whether anything can be recognised, and there SHALL be
nothing to confirm, accept or dismiss at capture time.

#### Scenario: Enter commits with no parser available
- **WHEN** the chord is used while nothing can parse the text
- **THEN** the capture is stored and the window closes
- **AND** the capture latency budget is unaffected

#### Scenario: A parse failure loses nothing
- **WHEN** parsing fails entirely for a capture
- **THEN** the note is still stored, searchable and surfaced

### Requirement: Three sigils name something with a page

`@`, `^` and `#` SHALL produce an entity mention. `>`, `!` and `~` SHALL NOT.

The division is what the sigil is *about*. A person, a project and a tag are
things a note can be *about*, and a page of everything naming one is meaningful.
A date, a priority and a duration are properties of the note itself — a page of
"everything due Friday" is the agenda, and a page of "everything urgent" is a
query over a field that is not even stored.

A sigil with nothing after it SHALL produce no mention, so that a bare `#` and a
`#?` name nothing rather than naming the empty string.

An address SHALL NOT be read as a mention. A sigil SHALL open a token to count,
which is what keeps `sarah@example.com` from naming a person called
`example.com`.

#### Scenario: A person, a project and a tag
- **WHEN** a capture reads `ask @sarah about ^migration #work`
- **THEN** three mentions SHALL be recorded

#### Scenario: A date is not an entity
- **WHEN** a capture carries `>friday`, `!p1` or `~30m`
- **THEN** no mention SHALL be recorded for it

#### Scenario: A bare sigil names nothing
- **WHEN** a capture reads `what is this #?`
- **THEN** no mention SHALL be recorded

#### Scenario: An email address names nobody
- **WHEN** a capture contains `sarah@example.com`
- **THEN** no mention SHALL be recorded

### Requirement: Items may be written with or without a tag

A capture SHALL be read as a checklist when it writes items, whether or not it
names a tag.

The guards that keep the form from firing inside ordinary writing SHALL stand:
the form is anchored at the start of the capture, a tag where one is written is
a single slug, and something must follow. A match firing inside prose is worse
than no match at all (§3.1.5).

#### Scenario: A tagged checklist
- **WHEN** a capture reads `#shop: milk, eggs, bread`
- **THEN** three items SHALL be recognised under the tag `shop`

#### Scenario: An untagged checklist
- **WHEN** a capture writes items with no tag
- **THEN** the items SHALL be recognised
- **AND** no tag SHALL be recorded

#### Scenario: Prose is still prose
- **WHEN** a capture reads `ask sarah about #shop: she has it`
- **THEN** no checklist SHALL be recognised

#### Scenario: A clock time is not a tag and a colon
- **WHEN** a capture reads `#standup at 9:30`
- **THEN** no checklist SHALL be recognised

### Requirement: A capture that nearly matched a form says so

Where a capture misses the list form by one thing, the system SHALL be able to
say what was missing. Recognising a near miss SHALL NOT make it a match.

§3.1.5 keeps the guards strict because a form firing inside ordinary writing is
worse than one that never fires. That argument is about *acting* on a guess.
Saying "this looks like a list you meant" costs a glance, which is the price
§3.1.3 already sets for a suggestion, and the alternative is what happened: a
grocery list written as one, not recognised, and nothing anywhere saying why.

Two misses SHALL be recognised and no others:

- an anchored tag with no colon, where a comma appears after it;
- an anchored tag with a colon and nothing after it.

A near miss SHALL NOT be reported where the capture already is a list, and SHALL
NOT be reported for text that is not anchored with a tag.

#### Scenario: A tag, commas, and no colon
- **WHEN** a capture reads `#groceries 7 apples, rye flour, 1kg lentils`
- **THEN** a near miss SHALL be available, naming the missing colon
- **AND** the capture SHALL still be a tagged note and not a list

#### Scenario: A colon with nothing after it
- **WHEN** a capture reads `#groceries:`
- **THEN** a near miss SHALL be available
- **AND** the capture SHALL still be a tagged note

#### Scenario: A list is not a near miss
- **WHEN** a capture reads `#groceries: milk, eggs`
- **THEN** no near miss SHALL be reported

#### Scenario: Prose is not a near miss
- **WHEN** a capture reads `ask sarah about the invoice, she has it`
- **THEN** no near miss SHALL be reported

#### Scenario: A tag with no commas is an ordinary note
- **WHEN** a capture reads `#reading finished the long one`
- **THEN** no near miss SHALL be reported

### Requirement: A sigil names the date and a verb names the kind

A task verb at the head of a capture SHALL compose with a date named by a sigil,
producing a task due on that date rather than an event occupying it.

The sigil rule is unchanged: where an explicit sigil and an inference disagree
about *which date*, the sigil wins. That rule answers a different question from
this one. §3.6 separates a thing that occupies time from a thing that is owed,
and the verb is what says which — so reading the sigil and ignoring the verb
answers only half of what was written.

This matters most where it is least expected: the more precisely somebody writes
a date, the more certainly they land on the wrong half of the table.

#### Scenario: A verb and a sigil date
- **WHEN** a capture reads `pay the invoice >2026-09-20`
- **THEN** it SHALL be a task due 2026-09-20
- **AND** SHALL NOT be an event

#### Scenario: A sigil date with no verb is unchanged
- **WHEN** a capture reads `dentist >2026-09-20`
- **THEN** it SHALL be an event on that day

#### Scenario: The sigil still decides which date
- **WHEN** a capture names one date with a sigil and another in prose
- **THEN** the sigil's date SHALL be the one used

#### Scenario: Confidence is unchanged
- **WHEN** a date is named by a sigil
- **THEN** the recognition SHALL remain fully confident

### Requirement: A date written in full is read wherever it appears

A complete date written as `YYYY-MM-DD` SHALL be recognised in a capture's text,
not only behind a sigil.

An unambiguous date that the archive cannot see is the worst of both: nothing is
guessed, and nothing is recorded either. It composes with a task verb like any
other date.

#### Scenario: A bare date in text
- **WHEN** a capture reads `dentist 2026-09-20`
- **THEN** it SHALL be recognised as that day

#### Scenario: A bare date with a verb
- **WHEN** a capture reads `pay the invoice on 2026-09-20`
- **THEN** it SHALL be a task due that day

#### Scenario: Something that is not a date is not read as one
- **WHEN** a capture contains `2026-13-45`
- **THEN** no date SHALL be recognised from it

### Requirement: A month name is read either way round

A day and a month name SHALL be recognised in either order: `sep 20` and
`20 sep` SHALL name the same day.

Day-first is how most of the world writes a date, and reading only one order
means the form silently works for some people and not others.

#### Scenario: Day before month
- **WHEN** a capture reads `pay the invoice on 20 sep`
- **THEN** it SHALL be a task due on the twentieth of September

#### Scenario: Month before day
- **WHEN** a capture reads `pay the invoice on sep 20`
- **THEN** it SHALL name the same day as the form above

#### Scenario: A number that is not a day
- **WHEN** a capture reads `pay 40 sep` with a number no month has
- **THEN** no date SHALL be recognised from it

### Requirement: An ambiguous numeric date is not guessed at

A date written only in digits and separators SHALL NOT be read as a date, such
as `20/09` or `20.09.2026`.

`09/20` and `20/09` are the same characters meaning different days, and nothing
in the capture says which convention was meant. §3.1.5 rates a match that fires
wrongly as worse than no match at all, and this is the case where being wrong is
invisible — a date off by months, recorded confidently.

#### Scenario: A slashed date names nothing
- **WHEN** a capture reads `pay the invoice on 20/09`
- **THEN** no date SHALL be recognised

#### Scenario: A dotted date names nothing
- **WHEN** a capture reads `pay the invoice on 20.09.2026`
- **THEN** no date SHALL be recognised

