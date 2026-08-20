# Plumb verdict: GO (run 20260820T020000Z)

## Lens poll
- cockpit-work / breakage: reported
- cockpit-work / intent: reported
- cockpit-work / motion: reported

## Findings (6)
- [MAJOR] intent / Centre pane, the ttui WORK table in both bottom frames - the horizontal band between the state column ('open'/'draft') and the 'verifiable' column, on all four rows (#140, #141, >142, >143) — Only two em-dash columns appear where the statement says three autonomy columns should each read as an em dash.
  scenario: cockpit-work
  evidence: Every row reads '#140  open  -  -  verifiable  ...'. Measured on the character grid the em dashes occupy exactly one cell each, at columns 33 and 44; columns 46 through 58 are entirely unlit black in rows #141, >142 and >143 (and in the inverted highlight of the #140 row), with the next glyph being the 'v' of 'verifiable' at column 59. So either the third autonomy column is rendering blank, or the third autonomy column is the one reading 'verifiable' - in which case it is not an em dash. Identical in both lower frames, so it is not a transient paint.
  confidence: high
- [MINOR] breakage / Bottom two frames, right-hand WORK pane, far right edge of the title column - row 2 (#141) and row 1 (#140), immediately left of the pane's right border — Work-item titles are hard-clipped at the pane's last usable column with no ellipsis or any other truncation marker, cutting a word in half.
  scenario: cockpit-work
  evidence: Row #141 reads "what should a Sparkline do with a singl" - the final glyph is a bare "l" occupying the last character cell before the vertical border line, with no following ellipsis character. Row #140 ends "...catalogue for missing" flush against the same edge, leaving the phrase dangling. Rows #142 and #143, whose titles are short, end well inside the pane, so the edge is a clip boundary rather than the end of the string.
  confidence: high
- [MINOR] motion / Frame 2 (top-right) - the '** BLOCKER **' text in the SOURCES box title at the bottom, read against the PROJECTS list at top-left and the 'nothing in flight' body above it — In the mid-transition frame, a BLOCKER banner is raised while the selected project and the visible pane give the viewer nothing that accounts for it.
  scenario: cockpit-work
  evidence: Frame 2 shows 'ok sesh' highlighted in PROJECTS, the right pane headed 'sesh' with only 'nothing in flight', and the footer reading 'work 0s . sessions live' - yet the SOURCES box title already carries '** BLOCKER **'. The cause appears to be ttui, which flips from 'ok ttui' (frame 1) to '!! ttui' in the same frame but is not the selected project and whose work list does not appear until frame 3. Should the blocker marker be legible as belonging to an unselected project, or is a viewer meant to read it as applying to the sesh view in front of them?
  confidence: medium
- [MINOR] motion / Frames 3 and 4 - the entire bottom-left and bottom-right panes of the contact sheet — The last two frames are identical, so the run's only substantive transition happens off-camera between frames 2 and 3 and is never sampled.
  scenario: cockpit-work
  evidence: Frames 3 and 4 are indistinguishable down to glyph placement: same 'ttui [WORK]' header, same four rows #140/#141/>142/>143, same 'unmapped:' line, same 'work 0s . verification:perceptual live . artifacts[0] live . sessions live' footer. Frames 1 and 2 differ only in three text rows (the ttui badge and the two SOURCES lines). So the sheet spends one frame on a static end state and the change from an empty 'nothing in flight' sesh view to a populated ttui work list with three live subscriptions occurs entirely in the gap between frames 2 and 3 - a viewer sees the before and the after but never the motion.
  confidence: high
- [MINOR] breakage / The two bottom frames of the contact sheet (frames 3 and 4), entire frame area — Frame 4 appears to be an exact repeat of frame 3 - should the final capture be showing a change that never rendered?
  scenario: cockpit-work
  evidence: Every element matches between the two bottom frames with no detectable difference: same PROJECTS selection on "!! ttui", same tab bar "ttui [WORK] VERIFY ARTIFACTS SESSIONS", same four work rows (#140 selected/inverse, #141, >142, >143), same "unmapped:" line, same footer "work 0s . verification:perceptual live . artifacts[0] live . sessions live". A pixel-level comparison of the two frame regions yields zero differing pixels. By contrast frames 1->2 and 2->3 each show visible state changes, so the sheet's last step is the only one that advances nothing.
  confidence: medium
- [NIT] intent / Rightmost title column of the ttui WORK table, bottom two frames, top two rows (#140 and #141) — Long titles are clipped flush against the pane's right border with no ellipsis, so a truncated title cannot be told apart from a complete one.
  scenario: cockpit-work
  evidence: Row #141's title ends 'what should a Sparkline do with a singl' - cut mid-word at the last usable column before the pane border - and #140 ends 'audit the widget catalogue for missing', which reads as an unfinished phrase. The two rows below (>142 'feat(widgets): add a Gauge widget', >143 'chore: bump the MSRV') end well short of the border, so there is no visual cue distinguishing the two cases.
  confidence: medium

## Accounting
previously overruled (0)
0 finding(s) dropped for naming no region
deferred to a later batch: none
stale ruling(s) needing re-validation: none
