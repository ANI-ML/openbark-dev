//! Speaker labels from voice embeddings, without the models: where one
//! utterance changes voice ([`change_points`]), and every utterance's final
//! label once a recording is complete ([`recluster`]).
//!
//! Both are plain arithmetic on embeddings the caller computed, so they build
//! without sherpa-onnx and are tested with made-up vectors.

use crate::{MIN_EMBED_SECS, band_apart};

/// What the speaker model heard in one utterance (or piece of one).
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    /// The speaker embedding, unit length.
    pub embedding: Vec<f32>,
    /// Length of the audio, seconds.
    pub secs: f32,
    /// High-band level in dB ([`crate::high_band_db`]): a phone voice or a
    /// voice in the room.
    pub band: f32,
}

/// How [`change_points`] looks for a second voice inside an utterance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitOptions {
    /// Length of each embedding window, seconds.
    pub window_secs: f32,
    /// Step between windows, seconds.
    pub hop_secs: f32,
    /// No piece is shorter than this, seconds.
    pub min_piece_secs: f32,
    /// Cut where the voices just before and just after a word are less alike
    /// than this (cosine of the windows' mean embeddings).
    pub threshold: f32,
}

impl Default for SplitOptions {
    /// Measured on the speaker labels eval (ADR-0030).
    fn default() -> Self {
        Self { window_secs: 1.5, hop_secs: 0.75, min_piece_secs: 1.0, threshold: 0.45 }
    }
}

/// Where the voice changes inside one utterance of `len` samples (16 kHz),
/// as sample offsets chosen from `word_starts` (cuts fall between words, so
/// the recognised text divides there too). `embed(a, b)` embeds samples
/// `a..b`, unit length, or `None`.
///
/// Embeddings are taken over overlapping windows. A word start is a cut
/// when the windows ending just before it and those starting just after it
/// sound like different voices; the least alike go first, and no piece is
/// shorter than `min_piece_secs`. An utterance too short for a window and a
/// piece on each side is never cut.
pub fn change_points(
    len: usize,
    word_starts: &[usize],
    options: &SplitOptions,
    embed: &mut dyn FnMut(usize, usize) -> Option<Vec<f32>>,
) -> Vec<usize> {
    let rate = os_asr::SAMPLE_RATE as f32;
    let win = (options.window_secs * rate) as usize;
    let hop = ((options.hop_secs * rate) as usize).max(1);
    let min_piece = (options.min_piece_secs * rate) as usize;
    if win == 0 || len < 2 * min_piece.max(win) {
        return Vec::new();
    }
    let candidates: Vec<usize> =
        word_starts.iter().copied().filter(|&b| b >= min_piece && b + min_piece <= len).collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // Windows over the whole utterance, the last one ending at its end.
    let mut starts: Vec<usize> = (0..).map(|k| k * hop).take_while(|s| s + win <= len).collect();
    if starts.last().is_some_and(|&s| s + win < len) {
        starts.push(len - win);
    }
    let windows: Vec<(usize, Vec<f32>)> =
        starts.into_iter().filter_map(|s| Some((s, embed(s, s + win)?))).collect();
    let Some(dim) = windows.first().map(|(_, e)| e.len()) else { return Vec::new() };
    // A window may reach a little past the word on its side; each side takes
    // the windows within two window lengths of it.
    let slack = hop / 2;
    let reach = 2 * win;
    let side = |b: usize, before: bool| -> Option<Vec<f32>> {
        let mut sum = vec![0.0f32; dim];
        let mut any = false;
        for (s, e) in &windows {
            let end = s + win;
            let near =
                if before { end <= b + slack && s + reach >= b } else { s + slack >= b && end <= b + reach };
            if near {
                sum.iter_mut().zip(e).for_each(|(a, x)| *a += x);
                any = true;
            }
        }
        any.then_some(sum)
    };
    let mut scored: Vec<(usize, f32)> =
        candidates.iter().filter_map(|&b| Some((b, cosine(&side(b, true)?, &side(b, false)?)))).collect();
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut cuts: Vec<usize> = Vec::new();
    for (b, sim) in scored {
        if sim >= options.threshold {
            break;
        }
        if cuts.iter().all(|c| c.abs_diff(b) >= min_piece) {
            cuts.push(b);
        }
    }
    cuts.sort_unstable();
    // Each cut must part two different voices over the whole of the pieces
    // either side, not only near the cut: the most alike neighbours join
    // first, until every neighbour differs. (A piece too short to embed
    // can't be told apart, so it joins.)
    while !cuts.is_empty() {
        let bounds: Vec<usize> =
            std::iter::once(0).chain(cuts.iter().copied()).chain(std::iter::once(len)).collect();
        let pieces: Vec<Option<Vec<f32>>> = bounds.windows(2).map(|w| embed(w[0], w[1])).collect();
        let alike = (0..cuts.len())
            .map(|i| match (&pieces[i], &pieces[i + 1]) {
                (Some(a), Some(b)) => (i, cosine(a, b)),
                _ => (i, 1.0),
            })
            .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)));
        match alike {
            Some((i, sim)) if sim >= options.threshold => {
                cuts.remove(i);
            }
            _ => break,
        }
    }
    cuts
}

/// How [`recluster`] groups voices into speakers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterOptions {
    /// The number of speakers, when the clinic set it ("Speakers in the
    /// room"); `None` estimates it.
    pub speakers: Option<u32>,
    /// Estimating: two groups whose voices are at least this alike on
    /// average (cosine between their utterances) are one speaker.
    pub threshold: f32,
    /// The pull towards two speakers, most consults' number: groups merge
    /// down to two at `threshold - prior`, and the last two only at
    /// `threshold + prior`.
    pub prior: f32,
    /// Estimating, beyond two speakers: a speaker needs at least this much
    /// speech (seconds); less joins the nearest voice.
    pub min_speaker_secs: f32,
}

impl Default for ClusterOptions {
    /// Measured on the speaker labels eval (ADR-0030).
    fn default() -> Self {
        Self { speakers: None, threshold: 0.45, prior: 0.1, min_speaker_secs: 6.0 }
    }
}

/// The most voices grouped pairwise; longer recordings group their longest
/// utterances and match the rest to them (memory grows with the square).
const MAX_GROUPED: usize = 3000;

/// Speaker labels for a whole recording, one per entry of `voices` (in time
/// order), numbered by who speaks first; `None` where there's no voice.
/// Returns `None` when no utterance is long enough to group.
///
/// Utterances of a second or more are grouped bottom-up (average linkage,
/// each weighted by its length up to 10 s): the two most alike groups merge
/// until the number of speakers is reached, or, estimating, until the most
/// alike are less alike than the threshold for the next merge (see
/// [`ClusterOptions::prior`]). A phone voice and a voice in the room
/// ([`band_apart`]) merge only when nothing else can. Shorter utterances then
/// take the speaker whose voice is nearest.
pub fn recluster(voices: &[Option<Voice>], options: &ClusterOptions) -> Option<Vec<Option<u32>>> {
    let mut grouped: Vec<usize> =
        (0..voices.len()).filter(|&i| voices[i].as_ref().is_some_and(|v| v.secs >= MIN_EMBED_SECS)).collect();
    if grouped.is_empty() {
        return None;
    }
    if grouped.len() > MAX_GROUPED {
        grouped.sort_by(|&a, &b| voice_secs(voices, b).total_cmp(&voice_secs(voices, a)).then(a.cmp(&b)));
        grouped.truncate(MAX_GROUPED);
        grouped.sort_unstable();
    }
    let v = |i: usize| voices[grouped[i]].as_ref().expect("grouped voices exist");
    let n = grouped.len();
    let mut groups: Vec<Group> = (0..n)
        .map(|i| {
            let w = v(i).secs.min(10.0);
            Group { members: vec![i], weight: w, secs: v(i).secs, band: v(i).band * w }
        })
        .collect();
    let mut sim = vec![0.0f32; n * n];
    for i in 0..n {
        for j in i + 1..n {
            let s = cosine(&v(i).embedding, &v(j).embedding);
            sim[i * n + j] = s;
            sim[j * n + i] = s;
        }
    }
    let mut live = vec![true; n];
    // A phone voice and a room voice are ranked below every other pair.
    let rank = |groups: &[Group], sim: &[f32], a: usize, b: usize| -> f32 {
        let apart = band_apart(groups[a].mean_band(), groups[b].mean_band(), 10.0);
        sim[a * n + b] - if apart { 2.0 } else { 0.0 }
    };
    let nearest = |groups: &[Group], sim: &[f32], live: &[bool], a: usize| -> Option<(usize, f32)> {
        (0..n)
            .filter(|&b| b != a && live[b])
            .map(|b| (b, rank(groups, sim, a, b)))
            .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))
    };
    let mut best: Vec<Option<(usize, f32)>> = (0..n).map(|a| nearest(&groups, &sim, &live, a)).collect();
    // Average linkage: the merged group's likeness to another is its two
    // parts' likeness, weighted. Each group's most alike other group is kept
    // up to date, so a merge costs a pass over the groups, not over all pairs.
    let merge = |a: usize,
                 b: usize,
                 groups: &mut [Group],
                 sim: &mut [f32],
                 live: &mut [bool],
                 best: &mut [Option<(usize, f32)>]| {
        let (wa, wb) = (groups[a].weight, groups[b].weight);
        for k in (0..n).filter(|&k| live[k] && k != a && k != b) {
            let s = (wa * sim[a * n + k] + wb * sim[b * n + k]) / (wa + wb);
            sim[a * n + k] = s;
            sim[k * n + a] = s;
        }
        let gone = std::mem::take(&mut groups[b]);
        groups[a].absorb(gone);
        live[b] = false;
        best[b] = None;
        for k in (0..n).filter(|&k| live[k] && k != a) {
            best[k] = match best[k] {
                Some((j, _)) if j == a || j == b => nearest(groups, sim, live, k),
                Some((j, s)) => {
                    let r = rank(groups, sim, k, a);
                    if r > s { Some((a, r)) } else { Some((j, s)) }
                }
                None => nearest(groups, sim, live, k),
            };
        }
        best[a] = nearest(groups, sim, live, a);
    };
    let target = options.speakers.map(|k| (k.max(1) as usize).min(n));
    let mut count = n;
    while count > 1 && target.is_none_or(|k| count > k) {
        let Some((a, (b, s))) = best
            .iter()
            .enumerate()
            .filter_map(|(a, x)| x.map(|x| (a, x)))
            .max_by(|x, y| x.1.1.total_cmp(&y.1.1).then(y.0.cmp(&x.0)))
        else {
            break;
        };
        if target.is_none() {
            let need =
                if count > 2 { options.threshold - options.prior } else { options.threshold + options.prior };
            if s < need {
                break;
            }
        }
        merge(a.min(b), a.max(b), &mut groups, &mut sim, &mut live, &mut best);
        count -= 1;
    }
    // Estimating, beyond two: too little speech to be a speaker (a cough, a
    // bark, a word from the corridor) joins the nearest voice.
    if target.is_none() {
        while count > 2 {
            let Some(small) = (0..n)
                .filter(|&g| live[g] && groups[g].secs < options.min_speaker_secs)
                .min_by(|&x, &y| groups[x].secs.total_cmp(&groups[y].secs))
            else {
                break;
            };
            let Some((other, _)) = best[small] else { break };
            merge(small.min(other), small.max(other), &mut groups, &mut sim, &mut live, &mut best);
            count -= 1;
        }
    }
    // Each speaker's voice: the mean of its utterances, weighted.
    let speakers: Vec<(usize, Vec<f32>, f32)> = (0..n)
        .filter(|&g| live[g])
        .map(|g| {
            let dim = v(groups[g].members[0]).embedding.len();
            let mut sum = vec![0.0f32; dim];
            for &m in &groups[g].members {
                let w = v(m).secs.min(10.0);
                sum.iter_mut().zip(&v(m).embedding).for_each(|(s, e)| *s += w * e);
            }
            (g, sum, groups[g].mean_band())
        })
        .collect();
    let mut group_of: Vec<Option<usize>> = vec![None; voices.len()];
    for g in (0..n).filter(|&g| live[g]) {
        for &m in &groups[g].members {
            group_of[grouped[m]] = Some(g);
        }
    }
    // The rest: the nearest speaker, though not one it can't be by its band
    // (a short utterance needs the wider gap, as live).
    for (i, voice) in voices.iter().enumerate() {
        let Some(voice) = voice else { continue };
        if group_of[i].is_some() {
            continue;
        }
        let nearest = |allowed: &dyn Fn(f32) -> bool| {
            speakers
                .iter()
                .filter(|(_, _, band)| allowed(*band))
                .map(|(g, sum, _)| (*g, cosine(sum, &voice.embedding)))
                .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))
                .map(|(g, _)| g)
        };
        let gap = if voice.secs >= MIN_EMBED_SECS { 10.0 } else { 16.0 };
        group_of[i] =
            nearest(&|band: f32| !band_apart(voice.band, band, gap)).or_else(|| nearest(&|_: f32| true));
    }
    // Numbered in order of first speech.
    let mut number: Vec<Option<u32>> = vec![None; n];
    let mut next = 0u32;
    Some(
        group_of
            .into_iter()
            .map(|g| {
                let g = g?;
                Some(*number[g].get_or_insert_with(|| {
                    next += 1;
                    next - 1
                }))
            })
            .collect(),
    )
}

fn voice_secs(voices: &[Option<Voice>], i: usize) -> f32 {
    voices[i].as_ref().map_or(0.0, |v| v.secs)
}

/// Voices grouped as one speaker so far.
#[derive(Default)]
struct Group {
    /// Indices into the grouped voices.
    members: Vec<usize>,
    /// Sum of the members' weights.
    weight: f32,
    secs: f32,
    /// Weighted sum of the members' high-band levels.
    band: f32,
}

impl Group {
    fn mean_band(&self) -> f32 {
        self.band / self.weight.max(1e-9)
    }

    fn absorb(&mut self, other: Group) {
        self.members.extend(other.members);
        self.weight += other.weight;
        self.secs += other.secs;
        self.band += other.band;
    }
}

/// Cosine similarity (any lengths).
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb).max(1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: usize = os_asr::SAMPLE_RATE as usize;

    /// A unit vector near axis `axis` of 8, nudged by `jitter` along the next.
    fn unit(axis: usize, jitter: f32) -> Vec<f32> {
        let mut v = [0.0f32; 8];
        v[axis] = 1.0;
        v[(axis + 1) % 8] = jitter;
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    fn voice(axis: usize, jitter: f32, secs: f32) -> Option<Voice> {
        Some(Voice { embedding: unit(axis, jitter), secs, band: -10.0 })
    }

    #[test]
    fn a_turn_change_inside_an_utterance_is_cut_at_the_nearest_word() {
        // Voice A for 4 s, then voice B for 3 s; words every half second.
        let len = 7 * S;
        let words: Vec<usize> = (1..14).map(|k| k * S / 2).collect();
        let mut embed = |a: usize, b: usize| {
            // A window is the voice it mostly holds.
            let mid = (a + b) / 2;
            Some(if mid < 4 * S { unit(0, 0.1) } else { unit(3, 0.1) })
        };
        let cuts = change_points(len, &words, &SplitOptions::default(), &mut embed);
        assert_eq!(cuts.len(), 1, "{cuts:?}");
        assert!(cuts[0].abs_diff(4 * S) <= S / 2, "{cuts:?}");
    }

    #[test]
    fn one_voice_is_never_cut() {
        let words: Vec<usize> = (1..20).map(|k| k * S / 2).collect();
        let mut n = 0;
        let mut embed = |_: usize, _: usize| {
            n += 1;
            Some(unit(2, if n % 2 == 0 { 0.2 } else { -0.2 }))
        };
        assert!(change_points(10 * S, &words, &SplitOptions::default(), &mut embed).is_empty());
    }

    #[test]
    fn short_utterances_and_pieces_are_left_alone() {
        let mut embed = |a: usize, _: usize| Some(if a < S { unit(0, 0.0) } else { unit(4, 0.0) });
        // Too short for a window and a piece on each side.
        assert!(change_points(2 * S, &[S], &SplitOptions::default(), &mut embed).is_empty());
        // No word start far enough from either end.
        assert!(
            change_points(6 * S, &[S / 2, 6 * S - S / 4], &SplitOptions::default(), &mut embed).is_empty()
        );
        // No words at all (a recogniser without times).
        assert!(change_points(6 * S, &[], &SplitOptions::default(), &mut embed).is_empty());
    }

    #[test]
    fn two_voices_taking_turns_become_two_speakers_numbered_by_who_spoke_first() {
        let voices = vec![
            voice(5, 0.1, 3.0),
            voice(1, 0.2, 2.5),
            voice(5, -0.1, 4.0),
            voice(1, 0.0, 1.5),
            voice(5, 0.2, 2.0),
        ];
        let labels = recluster(&voices, &ClusterOptions::default()).unwrap();
        assert_eq!(labels, vec![Some(0), Some(1), Some(0), Some(1), Some(0)]);
    }

    #[test]
    fn one_voice_stays_one_speaker() {
        // A dictation: the same voice, a little different each time.
        let voices: Vec<Option<Voice>> =
            (0..12).map(|i| voice(2, (i % 5) as f32 * 0.15 - 0.3, 3.0)).collect();
        let labels = recluster(&voices, &ClusterOptions::default()).unwrap();
        assert!(labels.iter().all(|l| *l == Some(0)), "{labels:?}");
    }

    #[test]
    fn a_third_voice_with_enough_speech_is_a_third_speaker() {
        let mut voices = Vec::new();
        for _ in 0..4 {
            voices.extend([voice(0, 0.1, 3.0), voice(3, 0.1, 3.0), voice(6, 0.1, 3.0)]);
        }
        let labels = recluster(&voices, &ClusterOptions::default()).unwrap();
        assert_eq!(&labels[..3], &[Some(0), Some(1), Some(2)]);
        assert_eq!(labels.iter().flatten().max(), Some(&2));
    }

    #[test]
    fn a_stray_voice_with_little_speech_joins_the_nearest() {
        let mut voices = Vec::new();
        for _ in 0..5 {
            voices.extend([voice(0, 0.1, 3.0), voice(3, 0.1, 3.0)]);
        }
        // One 2 s word from someone else, nearer the first voice.
        voices.push(Some(Voice { embedding: unit(7, 0.3), secs: 2.0, band: -10.0 }));
        let labels = recluster(&voices, &ClusterOptions::default()).unwrap();
        assert_eq!(labels.iter().flatten().max(), Some(&1), "{labels:?}");
        assert_eq!(labels[10], Some(0));
    }

    #[test]
    fn the_clinic_s_speaker_count_is_used_as_given() {
        let mut voices = Vec::new();
        for _ in 0..3 {
            voices.extend([voice(0, 0.1, 3.0), voice(3, 0.1, 3.0), voice(6, 0.1, 3.0)]);
        }
        let two = ClusterOptions { speakers: Some(2), ..ClusterOptions::default() };
        let labels = recluster(&voices, &two).unwrap();
        assert_eq!(labels.iter().flatten().max(), Some(&1));
        // A dictation told there are two people gets two.
        let same: Vec<Option<Voice>> = (0..6).map(|i| voice(2, i as f32 * 0.05, 3.0)).collect();
        assert_eq!(recluster(&same, &two).unwrap().iter().flatten().max(), Some(&1));
        // More speakers than voices: one each.
        let four = ClusterOptions { speakers: Some(4), ..ClusterOptions::default() };
        assert_eq!(recluster(&same[..2], &four).unwrap(), vec![Some(0), Some(1)]);
    }

    #[test]
    fn a_phone_voice_and_a_room_voice_are_kept_apart() {
        // The model hears them as alike; the phone line says otherwise.
        let room = |j: f32| Some(Voice { embedding: unit(0, j), secs: 3.0, band: -8.0 });
        let phone = |j: f32| Some(Voice { embedding: unit(0, j + 0.3), secs: 3.0, band: -34.0 });
        let voices = vec![room(0.0), phone(0.0), room(0.1), phone(0.1), room(-0.1)];
        let labels = recluster(&voices, &ClusterOptions::default()).unwrap();
        assert_eq!(labels, vec![Some(0), Some(1), Some(0), Some(1), Some(0)]);
    }

    #[test]
    fn short_utterances_take_the_nearest_speaker_and_missing_voices_stay_unlabelled() {
        let voices = vec![
            voice(1, 0.0, 0.6), // before anyone long has spoken
            voice(5, 0.0, 3.0),
            None,
            voice(1, 0.1, 3.0),
            voice(5, 0.1, 0.5),
        ];
        let labels = recluster(&voices, &ClusterOptions::default()).unwrap();
        assert_eq!(labels, vec![Some(0), Some(1), None, Some(0), Some(1)]);
        assert_eq!(recluster(&[voice(1, 0.0, 0.5), None], &ClusterOptions::default()), None);
    }
}
