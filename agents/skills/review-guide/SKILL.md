---
name: review-guide
description: Write a reproducible PR reading/testing guide from the approved plan and recorded review evidence.
---

# Review guide output

Return Docs.body using these six headings:

## What changed and why

State the problem, resulting behavior and approved revision/hash. Cite only supplied links.

## Where to look first / riskiest parts

Give a short reading order: path → reason → relevant risk.

## How to test it yourself

List prerequisites, actual commands, expected behavior and safe isolated test setup.

## AI review rounds

Summarize recorded role/round, findings and their resolution. Do not infer a model name.

## Found during review

Summarize out-of-scope findings; link only actual supplied proposal numbers/URLs.

## Evidence

Tie executed checks/artifacts and known limits to the exact head. Separate local/mock success from unrun native/provider/playground steps.

The daemon adds verified Models used, same-family warnings, proposal links and closing reference. Never fabricate provenance, claim an unrun check, add Closes/Fixes references or edit/post a PR directly. Keep the guide concise enough for an owner to use.
