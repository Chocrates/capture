## ADDED Requirements

### Requirement: A list can be read from the command line

The command line SHALL print a list by its tag, showing each item in display
order with its completion state, followed by the tasks carrying that tag. It
SHALL take `--json` on the same terms as the other reading commands.

Where a tag names nothing, it SHALL say so rather than printing an empty
result.

#### Scenario: A list prints its items in order
- **WHEN** `capture list groceries` is run
- **THEN** the items appear in their display order
- **AND** each shows whether it is done

#### Scenario: A tag that names nothing says so
- **WHEN** a list is read for a tag nothing carries
- **THEN** it reports that nothing carries the tag

#### Scenario: The same answer as JSON
- **WHEN** `capture list groceries --json` is run
- **THEN** the items and tasks are emitted as JSON

### Requirement: An item can be completed from the command line

The command line SHALL complete and reopen a list item with `tick` and
`untick`, addressed by its list and the item's text. A separate verb SHALL be
used rather than overloading `done`, which already takes `<name>` and `<name>
<occ>`: telling those apart from a list and an item by whether the second
argument parses as a date is the fragile disambiguation §3.1.5's guards exist to
avoid. Where the text matches more than one item on that list, it
SHALL report the ambiguity rather than choosing.

#### Scenario: Buying the milk
- **WHEN** `capture tick groceries milk` is run
- **THEN** that item is marked done

#### Scenario: Putting it back
- **WHEN** `capture untick groceries milk` is run
- **THEN** that item is open again

#### Scenario: An ambiguous item is not guessed at
- **WHEN** two items on one list carry the same text
- **AND** one is completed by that text
- **THEN** the ambiguity is reported
- **AND** neither item is changed
