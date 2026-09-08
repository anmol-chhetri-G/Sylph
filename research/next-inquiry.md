# Next Inquiry — Phase 6

**Purpose:** Frame the unresolved Type 1 gaps as explicit suggestions for deeper investigation.

---

## Gap Map (Phase 5 Recap)

| Gap ID | Description | Type | Severity | Status |
|--------|-------------|------|----------|--------|
| G1 | No longitudinal neuroimaging studies on AI-cognition effects | Type 1 | CRITICAL | UNRESOLVED |
| G2 | No studies measure independent capability after AI removal | Type 1 | CRITICAL | UNRESOLVED |
| G3 | No fNIRS/fMRI data on "symbiotic" (daily habitual) users | Type 1 | HIGH | UNRESOLVED |
| G4 | Kosmyna preprint not peer-reviewed | Type 2 | HIGH | UNRESOLVED |
| G5 | All neuroimaging studies are acute (single session) | Type 1 | CRITICAL | UNRESOLVED |
| G6 | No non-Western samples in neuroimaging studies | Type 2 | MEDIUM | UNRESOLVED |
| G7 | No studies measure BCM theory predictions directly | Type 1 | HIGH | UNRESOLVED |
| G8 | Subjectivity gradient replication needed (Russell n=20) | Type 2 | HIGH | UNRESOLVED |

---

## Proposed Study #1: The Critical Test

### Title
"Neuroplastic Effects of Sustained AI Collaboration: A 12-Week Longitudinal fNIRS Study of Executive Function in Knowledge Workers"

### Rationale
This study addresses G1, G2, G3, G5, and G7 simultaneously. It is the single study that would most advance the field.

### Design
**Within-subjects, randomized crossover with washout period.**

### Participants
- N = 80 knowledge workers (25-45 years, regular computer use, no prior sustained AI assistant use)
- Power analysis: Based on Kosmyna's effect size (d = 0.85 for cortical connectivity), 80 participants provides >95% power for within-subjects comparisons at α = 0.05

### Conditions (within-subjects, counterbalanced)
1. **AI-Substitute:** Participants use AI for all writing/composition tasks (AI generates, participant edits minimally)
2. **AI-Scaffold:** Participants use AI as on-demand reference, must generate own drafts first (AI as tutor)
3. **Solo:** No AI assistance (baseline)

### Duration
- 4 weeks per condition, 12 weeks total
- 2-week washout between conditions
- Total participation: 16 weeks

### Measures

#### Primary (Neuroimaging)
- **fNIRS:** PFC activation (Δ[HbO], Δ[HbR]) during executive function tasks
  - Timepoints: Baseline (Week 0), Mid-condition (Week 2), End-condition (Week 4), Post-washout (Week 6/8/10), Final (Week 16)
  - Tasks: Stroop task, N-back working memory, verb generation (executive function battery)
- **Key comparison:** PFC activation at Week 4 vs. Baseline within each condition; and at Week 16 (after all conditions) vs. Baseline

#### Secondary (Behavioral)
- **Stroop test:** Inhibitory control (executive function)
- **N-back test:** Working memory capacity
- **Tower of London:** Planning and problem-solving
- **Remote Associates Test:** Creative synthesis
- **Custom writing task:** Independent writing quality (without AI)
- **Error detection task:** Ability to identify factual errors in AI-generated text

#### Tertiary (Self-report)
- NASA-TLX: Cognitive workload
- AI dependency scale (adapted from Zhang 2024)
- Critical thinking disposition scale
- Semi-structured interview at study end

### Key Hypotheses

| Hypothesis | Prediction | Test |
|-----------|------------|------|
| H1 (Atrophy) | AI-Substitute condition → PFC deactivation at Week 4 AND reduced behavioral performance on EF tasks at Week 16 | Compare PFC activation and behavioral scores: AI-Substitute Week 4 vs. Baseline; and AI-Substitute Week 16 vs. Baseline |
| H2 (Elevation) | AI-Scaffold condition → maintained PFC activation AND maintained/improved behavioral performance | Compare PFC activation and behavioral scores: AI-Scaffold Week 4 vs. Baseline; and AI-Scaffold Week 16 vs. Baseline |
| H3 (Efficiency vs. Atrophy) | AI-Substitute shows PFC deactivation BUT behavioral performance is maintained → efficiency; PFC deactivation AND behavioral decline → atrophy | Within-condition correlation of neural and behavioral changes |
| Subjectivity Gradient | PFC deactivation occurs for objective tasks but not subjective tasks within the same participants | Compare PFC activation during objective vs. subjective tasks across conditions |

### Analysis Plan
- Linear mixed models: DV ~ Condition * Timepoint + (1|Participant)
- Bayesian analysis to quantify evidence for/against null effects
- Mediation analysis: Does PFC change mediate behavioral change?
- Correlation: Is magnitude of PFC deactivation at Week 4 predictive of behavioral decline at Week 16?

### Expected Outcomes
- If H1 confirmed: AI-Substitute group shows PFC deactivation at Week 4 AND reduced Stroop/N-back scores at Week 16 (after AI removal)
- If H2 confirmed: AI-Scaffold group shows maintained PFC activation AND maintained/improved behavioral scores
- If efficiency: PFC deactivation occurs but behavioral scores are maintained regardless of condition
- If atrophy: PFC deactivation in AI-Substitute correlates with behavioral decline at Week 16

### What This Study Resolves
- G1: Longitudinal neuroimaging ✓
- G2: Independent capability after AI removal ✓ (behavioral tests at Week 16 without AI)
- G3: Sustained use (12 weeks) ✓
- G5: Multiple timepoints (not just acute) ✓
- G7: BCM predictions tested via PFC activation patterns ✓

---

## Proposed Study #2: The Subjectivity Gradient Replication

### Title
"Task Subjectivity as a Moderator of AI's Effects on Prefrontal Cortex Activation: A Pre-Registered Replication of Russell et al. (2025)"

### Rationale
Russell's finding (G8) is the most important empirical result in the field but has n = 20. Direct replication is essential.

### Design
- Between-subjects (AI vs. no-AI) × Task Type (objective, creative, subjective) mixed design
- N = 120 (40 per task type condition)
- Pre-registered on OSF

### Tasks
1. **Objective:** SAT-style reading comprehension (replicate Russell)
2. **Creative:** Poetry writing on standard theme (replicate Russell)
3. **Subjective:** Personal reflection on favorite album (replicate Russell)
4. **NEW — Episodic Memory:** Recall and describe a personal memory (extends Russell)

### Measures
- fNIRS (PFC activation): replicate Russell's exact protocol
- NASA-TLX: workload
- Performance quality: blind rating
- Enjoyment: Likert scale

### Key Test
Does the subjectivity gradient replicate in a larger, pre-registered sample?

---

## Proposed Study #3: Cross-Cultural Replication

### Title
"The Subjectivity Gradient Across Cultures: A Multi-Site fNIRS Study of AI-Cognition Interaction in Western and East Asian Knowledge Workers"

### Rationale
All neuroimaging studies are from Western populations (G6). Cultural differences in attitudes toward AI, education systems, and cognitive styles may moderate effects.

### Design
- Multi-site: USA (n=40), South Korea (n=40), India (n=40)
- Within-subjects: AI-assisted vs. solo writing
- Same tasks, same fNIRS protocol, same measures

### Key Test
Does the subjectivity gradient and the cortical deactivation effect replicate across cultures?

---

## Implementation Timeline

| Study | Prep | Data Collection | Analysis | Publication |
|-------|------|-----------------|----------|-------------|
| Study 1 (Longitudinal) | 3 months | 4 months (16 weeks + recruitment) | 3 months | 3 months |
| Study 2 (Replication) | 2 months | 2 months | 2 months | 2 months |
| Study 3 (Cross-cultural) | 4 months | 3 months | 3 months | 3 months |

**Total estimated time to resolve core gaps: 2-3 years**

**Priority order:** Study 1 is the single most important study. It should be funded and initiated immediately. Studies 2 and 3 can proceed in parallel.

---

## Funding Implications

- Study 1 requires: fNIRS equipment ($50-100K), participant compensation ($80 × 80 = $6,400), research staff (2 RAs × 4 months), PI time
- Estimated budget: $200,000 - $350,000
- Appropriate funding sources: NSF EHR (education research), NIH NICHD (cognitive development), NIMH (cognitive neuroscience), private foundations (e.g., Bill & Melinda Gates Foundation for education AI)
