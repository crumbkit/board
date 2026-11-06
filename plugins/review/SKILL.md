---
name: review
description: 'The review plugin records a code review as a brief and a report in the board. Use it when you review a change, and when you ask for a review of your own. Write the brief with `board -C <board> put /reviews/<id>/brief`, then run `board -C <board> call review.run --brief /reviews/<id>/brief`, or write the report yourself at `/reviews/<id>/report`. SKILL.md and README.md in the plugin''s directory say more.'
---

# review

A review is two records in the board under `/reviews/<id>/`: the brief,
which says what to review, and the report, which says what the review
found. `<id>` names the review, such as the change's name; a review of a
later head takes a new id.

## Which board

Your instructions name the board. If they name none, use the board you are
in.

## Steps

1. Write the brief, a `review/brief/v1`. `range` is `<base>..<head>` with
   both full commit ids, such as the merge base with main and the branch
   head. `focus` says what matters and what to check first. `questions`
   lists what the author wants answered, and `follows` names the previous
   report.

       echo '{"$type": "review/brief/v1", "range": "<base>..<head>", "focus": "<what to check first>"}' | board -C <board> put /reviews/<id>/brief

2. Run the review:

       board -C <board> call review.run --brief /reviews/<id>/brief

   It gives the brief and the diff to the agent that `BOARD_REVIEW_AGENT`
   names and writes the report at `/reviews/<id>/report`. When it fails
   with `review::agent::not_set` or `review::agent::failed`, have a
   reviewer that has not seen the change, such as a fresh subagent, review
   the diff, and write its report in step 3.
3. Write the report, a `review/report/v1`, whenever a review ran outside
   `review.run`, including one that found nothing:

       echo '{"$type": "review/report/v1", "brief": "/reviews/<id>/brief", "range": "<base>..<head>", "patch_ids": ["<patch id>"], "findings": [{"severity": "high", "text": "<what is wrong>", "file": "<path>", "line": 12}], "verdict": "changes", "by": {"program": "claude-code"}}' | board -C <board> put /reviews/<id>/report

   Each finding has `severity` (`high`, `medium` or `low`) and `text`, and
   `file` and `line` when it is about one place. `verdict` is `approve`,
   `changes` or `comment`. `by` names the reviewer. `patch_ids` lists the
   patch id of each commit in the range that has one, oldest first. Run
   this in the repository to print them:

       for c in $(git rev-list --reverse <base>..<head>); do GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git diff-tree -p --no-renames --no-ext-diff --no-textconv --full-index "$c" | GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git patch-id --verbatim | cut -d' ' -f1; done

4. When the review belongs to a change, attach the report:

       board -C <board> call changes.attach --path /changes/<name> --ref /reviews/<id>/report

   The change's review gate passes when an attached report's range ends at
   the change's head, its verdict is not `changes`, and it has no high
   finding. After you fix findings, record the new head and review it again
   under a new id, with `follows` naming the last report.

`board -C <board> call review.post --report /reviews/<id>/report --pr <url>`
comments a report on a pull request once the owner approves the comment.
`board -C <board> call review.covered --base <commit>` checks that committed
reports cover every commit from the base to HEAD. The plugin's README.md
gives every field, and how to review a record instead of commits.
