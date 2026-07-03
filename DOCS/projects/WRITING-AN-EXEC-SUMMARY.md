# SOP: Writing an Executive Summary

> A standard operating procedure for producing the **executive summary** on a
> project or issue timeline in dev-pulse. Written for anyone who has to brief a
> stakeholder who will read *only this section* and nothing else.

---

## 0. What an exec summary is (and isn't)

An executive summary is a **standalone** account of where a project stands,
written for a reader with two minutes and no context. It is **not**:

- a changelog (don't list every commit),
- a status ceremony ("we had 3 meetings"),
- a promise ("we hope to…") — state facts and dated commitments only.

If the reader takes away one sentence, it should be your first sentence.

---

## 1. The BLUF rule

**Bottom Line Up Front.** Lead with the answer, then support it.

> ✅ "ABC-TEST ships on schedule for 2026-08-15; one blocker (payment sandbox
> access) is being escalated this week."
>
> ❌ "This quarter the team worked on several initiatives across the backend…"

The first sentence must answer: **on track / at risk / off track — and why.**

---

## 2. The five-part structure

Keep it to five short blocks. Each is 1–3 sentences.

| # | Block | Answers |
|---|-------|---------|
| 1 | **Status** | On track, at risk, or off track? By when? |
| 2 | **Progress** | What materially got done since last report? |
| 3 | **Risks & blockers** | What could derail it? Who owns each? |
| 4 | **Decisions needed** | What do you need *from the reader*? |
| 5 | **Next steps** | The 2–3 concrete moves before the next update. |

Block 4 is the one people forget. If you need nothing, say "No decisions
needed" — silence reads as "everything's fine," which is rarely true.

---

## 3. Rules of thumb

1. **One screen, max.** ~150–250 words. If it scrolls, it's a report, not a
   summary.
2. **Dates, not adjectives.** "Delayed two weeks to 2026-08-29" beats
   "significantly delayed."
3. **Name owners.** Every risk and next step gets a person, not a team.
4. **Quantify.** "8 of 12 issues closed" beats "good progress."
5. **Write it last.** Draft the detail, then distill. You can't summarize what
   you haven't thought through.
6. **Cut hedging.** Delete "somewhat," "fairly," "we believe." Commit or flag.

---

## 4. Template

```
STATUS: <On track | At risk | Off track> — <headline in one line, with date>.

PROGRESS: <What materially advanced since the last summary. 1–3 sentences,
quantified.>

RISKS/BLOCKERS: <Top 1–3 risks. Each: what it is, impact, owner, mitigation.>

DECISIONS NEEDED: <What you need from the reader, or "None.">

NEXT STEPS: <2–3 concrete, owned, dated actions.>
```

---

## 5. Worked example

```
STATUS: At risk — ABC-TEST targeted for 2026-08-15 but the payment
integration is trending one week late.

PROGRESS: Checkout, cart, and auth flows complete (9 of 12 issues closed).
Load testing passed at 2× projected peak.

RISKS/BLOCKERS: Payment sandbox access still pending from the vendor
(owner: ap@nube-io.com); blocks the final integration test. Escalated to
vendor account manager 2026-07-01.

DECISIONS NEEDED: Approve a one-week slip to 2026-08-22 if sandbox access
isn't granted by 2026-07-10.

NEXT STEPS:
- ap@ chases vendor daily until sandbox is live (by 2026-07-10).
- QA drafts the payment test plan in parallel (by 2026-07-08).
- Reconfirm launch date at next update (2026-07-10).
```

---

## 6. Checklist before you publish

- [ ] First sentence states status + date.
- [ ] Under 250 words.
- [ ] Every risk and next step has a named owner.
- [ ] Numbers instead of adjectives wherever possible.
- [ ] "Decisions needed" is explicit (even if "None").
- [ ] A reader who saw nothing else would know what to do next.
