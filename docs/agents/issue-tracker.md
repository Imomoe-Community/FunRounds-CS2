# Issue tracker: GitHub

Issues and specs live in Imomoe-Community/FunRounds-CS2.
Use the gh CLI from this repository.

- Create: gh issue create --title "..." --body "..."
- Read: gh issue view <number> --json number,title,body,labels,comments
- List: gh issue list --state open
- Comment: gh issue comment <number> --body "..."
- Label: gh issue edit <number> --add-label "..."
- Remove label: gh issue edit <number> --remove-label "..."
- Close: gh issue close <number> --comment "..."

“Publish to the issue tracker” means create a GitHub issue.
“Fetch the relevant ticket” means read the GitHub issue.

## Pull requests as a triage surface

**PRs as a request surface: no.**
