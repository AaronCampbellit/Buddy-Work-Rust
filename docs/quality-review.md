# Buddy beta.54 judging panel

Three independent reviewers assessed four categories on Windows 11 x64. The previous panel rated beta.53 **3.25/5**. Beta.54 scored **3.5/5**, weighting each category equally. A score of 3 means usable with noticeable issues, 4 means good with minor gaps, and 5 means excellent.

| Category | Beta.53 | Beta.54 | Evidence |
|---|---:|---:|---|
| Speed | 3 | 3 | Repeat visits improved in both profiles. Large Notes returns now reuse the accessible native list and rich editor. |
| Efficiency | 4 | 4 | Unchanged pinned previews and no-op saves avoid whole-collection work. Installed size is 45.45 MB. |
| Fluidity | 3 | 3 | The eight-switch release walkthrough showed complete destinations. See the longer stress runs and island results below. |
| Interaction and readability | 3 | 4 | The original three titles are legible in the dark workspace. A rendered test distinguished 20-character same-prefix titles at 100%, 125% and 150% app zoom. Header tools now expose a current-page name, persistent selection mark and tooltips. |

## Responsiveness

Each release ran 50 physical tab switches per profile in the same sequence. The small profile contained three notes and three clipboard items. The large profile contained 1,000 notes and 27 clipboard items, including 25 images. The production binary ran with an isolated, encrypted profile. Compiles, packaging, and the native test suite were stopped during this comparison. The following results exclude the first visit to each of the five pages. Times run from mouse release to settled desktop pixels. Tail figures use the nearest-rank 95th percentile.

| Interaction | Repetitions per build | Beta.53 median / p95 | Beta.54 median / p95 |
|---|---:|---:|---:|
| Small profile, warm tabs | 45 | 445 / 631 ms | 242 / 353 ms |
| Large profile, warm tabs | 45 | 947 / 3,691 ms | 263 / 695 ms |
| Large profile, return to Notes | 9 | 3,597 / 3,798 ms | 177 / 241 ms |

Cold visits are still slower. The longest first-round switch on the large profile took 2,706 ms. The warm-navigation targets are 150 ms median and 250 ms p95.

## Motion and input

Across twenty open, close, and reversal cycles per release, the desktop sampler measured gaps between visible width changes while the island moved. These samples used Windows desktop duplication with video encoding disabled. The sampler can coalesce frames. Desktop, GPU, profile and Windows scaling can affect timings.

| Gesture | Beta.53 median / p95 gap | Beta.54 median / p95 gap |
|---|---:|---:|
| Open | 110.7 / 255.6 ms | 79.9 / 232.7 ms |
| Close | 32.6 / 82.4 ms | 33.1 / 76.9 ms |
| Reverse | 39.5 / 64.7 ms | 33.7 / 64.4 ms |

All 20 reversal clicks reopened the island without resetting it. Opening still looks stepped. The beta.54 p95 gap between visible size changes was 233 ms against a 33 ms target. The longest reversal gap rose from 114 ms to 160 ms.

All 100 stress-test switches settled on the requested page, and the eight-switch installed walkthrough showed no partial frames above the detector’s 0.5% page-area threshold. The longer large-profile test revealed three real partial redraws lasting 33–34 ms each. One briefly clipped a Quick Access button and two blanked a Clipboard thumbnail. Lowering the threshold to 0.2% exposed a 58 ms strip on one small-profile Clipboard thumbnail. None showed the wrong page header.

## Resources and editing

In the 60-second normal-profile sample, the running Buddy process used 6.12 MiB of private memory and 13.31 MiB median working set. Its CPU use averaged 3.40% of one core. The previous sample measured 17.29 MiB private and 2.57% of one core. Uptime and the process’s active features were uncontrolled, so those samples do not establish a long-term memory or battery trend.

In matched Windows debug test binaries on a private desktop with 1,000 synthetic notes, 300 warm preview checks took 10.1 s in beta.53 and 1.56 ms in beta.54. The median unchanged save fell from 572 ms to 1.76 ms across 12 edits. Genuine edits still encrypt and durably replace the state file synchronously. That took a median 180 ms and peaked at 551 ms. The test reopened it and verified every edit. These timings measure the private test harness and its synthetic profile.

The installer upgraded the registered app from beta.53 to beta.54 and verified every packaged file. The encrypted profile and other Buddy shortcuts survived the upgrade. All 63 native tests passed, including scrolling, keyboard focus, rich text, DPI-aware layout and editing pinned notes.

## Remaining work

1. Profile island opening frame by frame. Measure capture, resize and presentation separately, then repeat the 20-reversal test against a 33 ms p95 visible-frame gap.
2. Reduce layout and frame-capture work in Clipboard with many images. Warm visits took a median 669 ms. Investigate the first Widgets visit separately, which took 2.71 seconds on the large profile. Recheck the thumbnail and toolbar redraws on repeated navigation.
3. Improve the clipped date and low-contrast pasted heading in the dark editor. Verify keyboard and Narrator announcements, high contrast and moving the app between displays at different Windows scales. The three tested scales were Buddy’s own zoom levels.

[Windows release, installer and recording](https://github.com/AaronCampbellit/Buddy-Work-Rust/releases/tag/v0.1.1-beta.54)
