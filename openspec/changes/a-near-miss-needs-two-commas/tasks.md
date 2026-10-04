# Tasks

## 1. The spec

- [ ] 1.1 Amend the requirement to say at least two commas, carrying the
      measurement as the reason rather than the bare number.
- [ ] 1.2 Add the boundary scenario: `#health slept badly, again` reports no
      near miss.

## 2. Check the code already matches

- [ ] 2.1 Confirm by running, not by reading: `#health slept badly, again` and
      `#groceries milk, eggs` report nothing; `#groceries 7 apples, rye flour,
      1kg lentils` reports the missing colon.
- [ ] 2.2 Add the one-comma case to the test suite if it is not already there —
      the existing tests use three items, which is why this drifted unseen.
- [ ] 2.3 Run `make ci`.
