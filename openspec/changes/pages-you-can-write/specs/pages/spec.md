## ADDED Requirements

### Requirement: A page is never read for dates or types

A note made a page SHALL NOT be promoted to a task, event or series by any
parse, and editing its text SHALL NOT re-read it. A page SHALL NOT appear on the
calendar or the task list. A page MAY be turned back into a capture, after which
its text is read as a capture's is.

#### Scenario: A routine that mentions a time
- **WHEN** a page titled "gym routine" is edited to contain "every monday at 2pm"
- **THEN** it remains a page
- **AND** no series exists for it and the calendar shows nothing from it

#### Scenario: Back to a capture
- **WHEN** that page is turned back into a capture
- **THEN** it is read as one and becomes a weekly series

### Requirement: A page starts from a title or from a capture

"New page" SHALL take a title, create a note whose first line is that title,
make it a page, and name it by the title's slug form where that name is free.
"Make this a page" SHALL make any existing capture a page without changing its
text.

#### Scenario: A new page is named by its title
- **WHEN** a new page is made with the title "Car maintenance"
- **THEN** `[[car-maintenance]]` resolves to it
- **AND** it is a page

#### Scenario: The name is taken
- **WHEN** a second page is made with the title "Car maintenance"
- **THEN** it is created with its slug as its name
- **AND** the surface says the name was in use

### Requirement: Checklist lines in a page are ticked by line

In a page, a line beginning `- `, `* `, `[ ]`, `[x]` or `[X]` SHALL be a
checklist item. Ticking an item on the page SHALL NOT change the text. A tick
SHALL follow its item when lines are reordered.

#### Scenario: Ticked without an edit
- **WHEN** "- passport" on a packing page is ticked
- **THEN** the page's text is unchanged
- **AND** the item shows as done on every device
