# Contradiction Matrix — Phase 3

**Protocol:** "No claim without a corpse. Every claim must survive at least one repulsion attempt."

---

## C1: Cortical Deactivation vs. Learning Enhancement

| Dimension | S2: Kosmyna et al. (2025) | S16: Nature Meta-analysis (2026) |
|-----------|---------------------------|----------------------------------|
| **Claim** | 55% reduction in cortical connectivity during AI-assisted writing vs. solo | ChatGPT improves student learning outcomes (g = 0.670, p < 0.001) |
| **Hypothesis** | Supports H1 (Atrophy) | Supports H2 (Elevation) |
| **Method** | Within-subjects EEG/fMRI, acute lab session | Meta-analysis of 35 experimental studies |
| **Population** | 54 healthy adults, 18-30 | Students across multiple educational contexts |
| **Task** | Writing with ChatGPT vs. brainstorming alone | Diverse: math, language, programming, science |
| **Duration** | Single session (~1 hour) | Studies ranged from days to weeks |
| **Measurement** | Brain connectivity (neural) | Task performance (behavioral) |
| **Key confound** | AI use measured during session only | Studies vary in AI integration method |

### Resolution Attempt

These sources do NOT actually contradict. They measure **different things at different layers of the Three-Layer Architecture**:

- Kosmyna measures **Layer 2** (functional brain activation during a task): The brain uses less energy when AI assists. This is consistent with *either* atrophy *or* neural efficiency.
- Nature Meta measures **Layer 1** (behavioral output): Students produce better answers with AI. This is consistent with *either* elevation *or* cognitive offloading.

**The real question is whether Layer 2 deactivation predicts Layer 1 degradation over time.** Neither source answers this. The contradiction is **apparent, not real** — but it reveals that the field conflates neural efficiency with cognitive health.

**Status:** UNRESOLVED. Requires longitudinal neuroimaging + behavioral testing.

---

## C2: Neural Efficiency vs. Cognitive Atrophy

| Dimension | S2: Kosmyna et al. (2025) | S25: Occupational Neuroplasticity Meta (2026) |
|-----------|---------------------------|------------------------------------------------|
| **Claim** | AI use reduces cortical connectivity | Expertise reduces brain activation in domain-specific regions |
| **Interpretation** | Atrophy (negative) | Efficiency (positive) |
| **Comparison population** | AI-assisted vs. solo writers | Experts vs. novices (taxi drivers, musicians, athletes) |
| **Duration** | Acute (single session) | Years of training |

### Resolution Attempt

The occupational neuroplasticity literature is the **strongest counterargument to H1 (Atrophy)**. Expert musicians show *reduced* cortical activation in motor regions during playing — this is interpreted as efficiency, not atrophy. The brain learns to do the same work with fewer resources.

**However, there is a critical disanalogy:**

- **Occupational expertise** involves years of *active practice* that strengthens domain-specific neural circuits. The reduced activation coexists with *enhanced* behavioral capability.
- **AI offloading** involves *passive delegation* that bypasses domain-specific circuits. The reduced activation has NOT been shown to coexist with maintained independent capability.

**The test:** If AI-induced deactivation is "efficiency," then AI users should perform *equally well or better* when AI is removed. If it is "atrophy," they should perform *worse*. **No study has measured this.**

**Status:** UNRESOLVED. This is the single most important empirical question in the field.

---

## C3: Speed Gains vs. Critical Thinking Erosion

| Dimension | S10: Spatharioti et al. (CHI 2025) | S11: Baldeo (APA 2026) | S12: Zhang et al. (2024) |
|-----------|--------------------------------------|------------------------|--------------------------|
| **Claim** | LLM search: 50% faster decisions, comparable accuracy when LLM correct | 58% of high-use adults report "AI did most of the thinking" | AI dependency → laziness, reduced critical/independent thinking |
| **Hypothesis** | Supports H2 (productivity) | Supports H1 (Atrophy) | Supports H1 (Atrophy) |
| **Method** | Randomized experiment (n=210) | Behavioral survey of high-use adults | Survey (n=300 university students) |
| **Population** | Online workers ( Mechanical Turk) | High-use AI adults | South Korean university students |
| **Task** | Product comparison decisions | General AI usage patterns | Academic AI usage |

### Resolution Attempt

Spatharioti shows that LLM search makes people **faster** but also makes them **blind to errors**. When the LLM hallucinated, accuracy dropped to 47% (vs. 93% for traditional search). Users did not verify or double-check incorrect LLM outputs.

This is **not elevation — it is expedited offloading with reduced error-detection.** Baldeo and Zhang confirm the downstream consequence: habitual use leads to "AI did the thinking" self-reports and reduced critical thinking.

**The contradiction dissolves when you separate speed from quality:** AI improves the former at the cost of the latter. H2 survives on speed; H1 survives on quality.

**Status:** PARTIALLY RESOLVED. Speed and quality are in tension. No source shows AI improves *both* simultaneously in the long term.

---

## C4: Educational Benefits vs. Cognitive Dependency

| Dimension | S15: Yuxian (2025) | S14: Farhat (2025) |
|-----------|--------------------|--------------------|
| **Claim** | LLM + critical thinking guidance → improved declarative AND procedural learning | Students become "hooked on help," anxiety during AI unavailability |
| **Hypothesis** | Supports H2 (Elevation) | Supports H1 (Atrophy) |
| **Method** | Experiment with controls | Qualitative interviews (n=40) |
| **Key variable** | Critical thinking guidance paired with AI | Unguided, habitual AI use |
| **Duration** | Controlled intervention | Longitudinal observation of naturalistic use |

### Resolution Attempt

**This is the most important contradiction in the dataset because it identifies the MODERATING VARIABLE.**

Yuxian's intervention paired AI with explicit critical thinking instruction. Farhat's students used AI without such scaffolding. The difference is not AI itself but **how AI is integrated into the learning process.**

- **Scaffolded AI** (Yuxian): AI as tool + human maintains critical oversight → Elevation possible
- **Unscaffolded AI** (Farhat): AI as substitute + human abdicates oversight → Atrophy likely

**H1 and H2 are not competing hypotheses — they describe different usage modes.** The field has been asking "Is AI good or bad for cognition?" when it should be asking "Under what conditions does AI enhance vs. erode cognition?"

**Status:** RESOLVED. The moderating variable is scaffolded vs. unscaffolded use. This reframes the entire research question.

---

## C5: Objective Task Benefits vs. Subjective Task Failure

| Dimension | S3: Russell et al. (2025) — SAT tasks | S3: Russell et al. (2025) — Reflection tasks |
|-----------|----------------------------------------|-----------------------------------------------|
| **Claim** | Copilot reduced workload, increased enjoyment, improved performance | No benefits reported; PFC activation unchanged |
| **Brain data** | PFC VLF power ↓ (deactivation) with Copilot | No significant PFC change with Copilot |
| **Task type** | Objective, fact-based (reading comprehension) | Subjective, personal (favorite album reflection) |
| **AI effectiveness** | Copilot provided correct, useful answers | Copilot could not meaningfully assist |

### Resolution Attempt

Within a single study, Russell demonstrates that **AI's cognitive effect is fundamentally moderated by task subjectivity.** The same tool, the same users, the same lab — but opposite outcomes depending on whether the task engages working memory (objective) or episodic memory (subjective).

This means:
- For **objective tasks**: AI reduces cognitive load (supports H2, but also consistent with H1 if the reduction leads to disuse)
- For **subjective tasks**: AI is irrelevant to cognition (partially supports H3 — brain rejects AI for personal meaning-making)

**Status:** RESOLVED within the study. The subjectivity gradient is an empirical finding, not a theoretical prediction.

---

## Summary of Contradictions

| ID | Contradiction | Status | Resolution |
|----|---------------|--------|------------|
| C1 | Cortical deactivation vs. learning enhancement | UNRESOLVED | Different layers of architecture; need longitudinal study |
| C2 | Neural efficiency vs. cognitive atrophy | UNRESOLVED | Critical disanalogy: expertise involves practice, AI involves delegation |
| C3 | Speed gains vs. critical thinking erosion | RESOLVED | Speed ↑ but quality ↓; no source shows both improve |
| C4 | Educational benefits vs. cognitive dependency | RESOLVED | Moderating variable: scaffolded vs. unscaffolded use |
| C5 | Objective task benefits vs. subjective task failure | RESOLVED | Subjectivity gradient is empirical finding |

**Unresolved contradictions require:** Longitudinal neuroimaging studies measuring both brain structure/function AND independent behavioral capability over time.
