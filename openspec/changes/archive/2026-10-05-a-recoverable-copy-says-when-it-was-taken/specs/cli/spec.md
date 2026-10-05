## ADDED Requirements

### Requirement: A recoverable copy is identified by when it was taken

A listing of recoverable copies SHALL say when each was taken, in prose and in
`--json`.

The id and the remaining retention identify a copy to the system; the date
identifies it to a person. These are plaintext copies of content somebody
ordered destroyed, and choosing between two of them means knowing which
destruction each belongs to — which "expires in 27d" answers only by arithmetic
the reader has to do, against a retention they would have to remember.

The listing SHALL continue to show the first line only. The purpose is
recognising a copy, not reading destroyed content back out of a listing.

#### Scenario: A listing says when each copy was taken
- **WHEN** copies are listed
- **THEN** each SHALL show when it was taken
- **AND** SHALL show what it begins with

#### Scenario: The machine-readable form carries it too
- **WHEN** copies are listed with `--json`
- **THEN** each SHALL carry when it was taken, as a number a consumer can use
