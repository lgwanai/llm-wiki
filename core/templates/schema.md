# Wiki Schema

> ⚠️ **This is the most important file in the wiki.** It transforms a generic LLM into a
> disciplined knowledge worker. Co-evolve this with the LLM over time. The first version
> will be rough — after a few dozen sources and lint passes, it will reflect how your
> domain actually works.

---

## Domain Context

<!-- Describe your project/domain. What are you building? What's the tech stack?
     What's the team structure? This helps the LLM understand context. -->

**Project**: [Project name and brief description]
**Tech stack**: [Languages, frameworks, infrastructure]
**Team**: [Roles and responsibilities]

---

## Entity Types

<!-- Define what types of entities exist in your domain. Add, remove, or modify
     types as your wiki evolves.

     PARSER TABLE (required for automated tools — keep synced with sections below):
     | type | directory | description |
     |------|-----------|-------------|

     Each type also has a detailed section below with required/optional attributes.
-->

| type | directory | description |
|------|-----------|-------------|
| `person` | entities | An individual who contributes to or is relevant to the project |
| `project` | entities | A named initiative, component, or system within the codebase |
| `library` | entities | An external dependency or tool used by the project |
| `concept` | concepts | An abstract idea, pattern, or architectural principle |
| `file` | entities | A significant source file or configuration file |
| `decision` | decisions | An architectural or design decision (ADR format) |
| `pattern` | patterns | A recurring code pattern, workflow, or convention observed |
| `tool` | entities | A development tool, service, or platform |

### person
**Description**: An individual who contributes to or is relevant to the project
**Required attributes**: name, role
**Optional attributes**: team, email, expertise, preferences
**Page template**: `templates/entity-page.md`

### project
**Description**: A named initiative, component, or system within the codebase
**Required attributes**: name, status
**Optional attributes**: repo, language, owner, dependencies
**Page template**: `templates/entity-page.md`

### library
**Description**: An external dependency or tool used by the project
**Required attributes**: name, version, purpose
**Optional attributes**: docs_url, alternatives, license
**Page template**: `templates/entity-page.md`

### concept
**Description**: An abstract idea, pattern, or architectural principle
**Required attributes**: name, domain
**Optional attributes**: definition, related_to, examples
**Page template**: `templates/entity-page.md`

### file
**Description**: A significant source file or configuration file
**Required attributes**: path, language
**Optional attributes**: purpose, last_modified, owner
**Page template**: `templates/entity-page.md`

### decision
**Description**: An architectural or design decision (ADR format)
**Required attributes**: title, date, status, decision
**Optional attributes**: context, rationale, alternatives, consequences
**Page template**: (inline ADR format)

### pattern
**Description**: A recurring code pattern, workflow, or convention observed
**Required attributes**: name, category, description
**Optional attributes**: frequency, confidence, examples, related_patterns
**Page template**: `templates/entity-page.md`

### tool
**Description**: A development tool, service, or platform
**Required attributes**: name, category
**Optional attributes**: vendor, cost, usage, alternatives
**Page template**: `templates/entity-page.md`

---

## Relationship Types

<!-- Define the typed relationships between entities. These become edges in the
     knowledge graph. Add types as you discover new relationship semantics. -->

| Type | Direction | Meaning | Example |
|------|-----------|---------|---------|
| `uses` | A → B | A depends on B for functionality | "Auth service uses Redis" |
| `depends_on` | A → B | A requires B (stronger than uses) | "API gateway depends_on Auth" |
| `owns` | A → B | A is responsible for B | "Sarah owns Auth Migration" |
| `contains` | A → B | A is composed of / includes B | "auth-service contains middleware.ts" |
| `implements` | A → B | A realizes pattern/interface B | "Rate limiter implements Token Bucket" |
| `contradicts` | A ↔ B | A conflicts with B | Two competing claims |
| `supersedes` | A → B | A replaces B | "Redis v7.0 supersedes v6.2" |
| `caused` | A → B | A led to B | "Missing index caused slow query" |
| `fixed` | A → B | A resolved B | "PR #456 fixed rate limiting bug" |
| `related_to` | A ↔ B | General connection | Fallback when type unclear |

---

## Ingest Rules

### Source Types and How to Handle Them

| Source Type | Processing | Confidence Start |
|------------|-----------|-----------------|
| Code files (scanned directly) | High. Extract imports, dependencies, config. | 0.9 |
| Official documentation | High. Extract facts, APIs, patterns. | 0.85 |
| Session conversation | Medium. Extract decisions, findings, observations. | 0.6 |
| External article / blog post | Medium. Extract claims. Note if unverified. | 0.5 |
| Meeting notes | Low. Extract decisions only. Rest decays fast. | 0.4 |
| Speculation / opinion | Low. Mark as unverified. Decays fast. | 0.3 |

### When to Create a New Page vs. Update Existing

- **New page**: When the entity doesn't exist in the graph, or the content is a
  distinctly new topic not covered by existing pages
- **Update existing**: When adding to or correcting an existing entity page
- **Session digest**: Always a new page. Each session gets its own digest.

### Quality Standards for New Content

- Every claim should cite at least one source
- Entity pages should use the appropriate template
- Relationships should use the most specific type available (not just `related_to`)
- Content should be consistent with existing wiki knowledge
- Sensitive data must be filtered before writing

---

## Query Rules

### How to Search

1. Parse the query intent (fact check, explanation, discovery, impact analysis)
2. Choose search strategy based on intent (keyword, vector, graph, hybrid)
3. Retrieve relevant pages and entities
4. Synthesize answer with citations and confidence scores
5. Choose output format based on query type

### When to File Answers Back

- Answer reveals new facts → ingest to working memory
- Answer clarifies existing knowledge → update entity pages
- Answer resolves a contradiction → record resolution
- Answer is a one-off → don't file (avoid noise)

Quality threshold for filing: confidence > 0.5 AND answer is non-trivial (>3 sentences).

---

## Quality Standards

### Required Fields for Entity Pages
- YAML frontmatter with: id, type, name, confidence, sources, status
- At least one paragraph of description
- At least one relationship (edge in the graph)
- Quality score above 0.4

### Quality Scoring Weights
- Structure: 20%
- Completeness: 20%
- Source citation: 15%
- Consistency: 20%
- Freshness: 10%
- Readability: 15%

### Auto-Heal Rules
- Orphan pages → auto-link to related entities
- Stale claims (> retention threshold) → mark as stale
- Broken wikilinks → find best match by edit distance
- Missing frontmatter → add with defaults, flag low confidence

---

## Consolidation Schedule

### Default Schedule
- **Working → Episodic**: Daily, or when ≥ 5 observations accumulate
- **Episodic → Semantic**: Weekly, or when same fact appears in ≥ 2 episodes
- **Semantic → Procedural**: Manual only (human approval required)

### Retention Decay Parameters
- Architecture decisions: half-life 180 days
- Project facts: half-life 90 days
- Bug reports: half-life 14 days
- Meeting notes: half-life 7 days
- Code patterns: half-life 60 days
- Personal preferences: half-life 365 days

---

## Privacy & Scope Defaults

### Sensitive Data Filtering
- API keys, tokens, passwords → redact on ingest (NEVER write to wiki)
- PII (emails, phones, addresses) → redact or hash
- Internal IPs → redact in shared wikis
- Connection strings → redact credentials, keep host/port

### Default Scope
- Session digests: `private` (by default, promote useful content to `team`)
- Entity pages: `team` (project knowledge is shared)
- Personal preferences: `private`
- Architecture decisions: `team`

---

## Co-Evolution Rules

### When to Update This Schema
- New entity type emerges → add to Entity Types section
- New relationship type proves useful → add to Relationship Types section
- Quality rules need tightening → update Quality Standards
- Consolidation schedule needs adjusting → update schedule
- Privacy rules need updating → update Privacy defaults

### Schema Version
Track changes with a version comment at the top:
```
<!-- schema-version: 1.3 | last-updated: 2024-04-02 | updated-by: sisyphus -->
```

### Review Cycle
- Review schema monthly
- After major wiki growth (>50 new pages) → review schema
- After integrating a new domain → review schema
