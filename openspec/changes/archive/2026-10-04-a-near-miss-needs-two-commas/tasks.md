# Tasks

## 1. The spec

- [x] 1.1 Amend the requirement to say at least two commas, carrying the
      measurement as the reason rather than the bare number.
- [x] 1.2 Add the boundary scenario: `#health slept badly, again` reports no
      near miss.

## 2. Check the code already matches

- [x] 2.1 Confirm by running, not by reading: `#health slept badly, again` and
      `#groceries milk, eggs` report nothing; `#groceries 7 apples, rye flour,
      1kg lentils` reports the missing colon. All three behave as stated, along
      with the other two lines from the measurement:

      ```
       #groceries 7 apples, rye flour, 1kg lentils  Some(NoColon { tag: "groceries" })
                            #groceries milk, eggs  None
                       #health slept badly, again  None
          #reading finished the long one, finally  None
             #journal a quiet day, mostly reading  None
                                      #groceries:  Some(NothingAfterTheColon { .. })
                           #groceries: milk, eggs  None
      ```
- [x] 2.2 Add the one-comma case to the test suite if it is not already there —
      the existing tests use three items, which is why this drifted unseen.
      **Already there, and better than I expected.**
      `one_comma_is_a_sentence_and_two_are_a_list` asserts all three prose lines
      report nothing and two three-item lists report something, and
      `a_two_item_list_without_its_colon_stays_quiet` writes down the cost of
      the rule — `#shop milk, eggs` is a real list that stays silent. So the
      *code* side of this boundary was pinned all along; only the document had
      drifted. Nothing added.
- [x] 2.3 Run `make ci`. Not re-run in full for this change: it alters no code,
      only `openspec/specs/recognizer/spec.md`. The parse suite was run and is
      green, and the full set was green on the commit immediately before this
      one apart from `deny`'s network failure.
