---
name: jira
description: 'Record a Jira issue as a typed jira/issue/v1 record that other records can reference. Use it when work names a Jira ticket. Start with `board -C <board> call jira.ref --key ACME-123 --site acme`, which writes /jira/acme/ACME-123 once with its title. SKILL.md and README.md in the plugin''s directory say more.'
---

# jira

## Record an issue

1. Run `board -C <board> call jira.ref --key <KEY>`. Add `--site <site>` when
   the board has no default site at `/jira/config`. The site is the name
   before `.atlassian.net`.
2. Reference the issue by the `path` it returns, such as `/jira/acme/ACME-123`,
   in the record that names the ticket.
3. When `title_note` is not null, the record has no title. Tell the person why,
   such as `acli` not being signed in. Do not write a title yourself.

Running `ref` again for the same issue writes nothing and returns the record
with `written` false.

## Codes

- `jira::key::invalid`: the key is not a project key, a hyphen and a number.
  Ask the person for the key as Jira shows it, such as `ACME-123`.
- `jira::site::invalid`: pass the site name alone, such as `acme`.
- `jira::site::missing`: pass `--site`, or ask the person which site to use.
- Any other code: tell the person the message.
