---
id: ct-z5pe
status: open
deps: []
links: []
created: 2026-02-06T12:07:02Z
type: bug
priority: 2
assignee: Jeffery Utter
tags: [needs-plan]
---
# Ensure project list is sorted by most recent activity

I'm not sure the work done in ct-pcrq was completed correctly. I don't see the most recent modified time after the project path in the list (like we do for sessions and agents), also the lists do not appear to be sorted. Lastly, looking at the code I think we're taking the modified timestamp of the session files, rather than reading it from the JSONL of the session itself. The time of the project should be the time of the most recent session which should be the time of the most recent agent.

