-- ============================================================================
-- Impulse Knowledge Base Schema
-- ============================================================================
-- Unified queryable database for all project documentation.
-- Replaces sparse markdown file searching with structured FTS5 queries.
--
-- Usage:
--   sqlite3 knowledge.db < schema.sql
--   python3 extract.py          # populate from docs/
--   sqlite3 knowledge.db "SELECT * FROM search('hooks architecture')"
-- ============================================================================

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ============================================================================
-- Core Tables
-- ============================================================================

-- Every markdown file in docs/
CREATE TABLE IF NOT EXISTS documents (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    path        TEXT    NOT NULL UNIQUE,          -- relative path from project root
    title       TEXT    NOT NULL,                 -- first H1 heading
    category    TEXT    NOT NULL,                 -- spec, decision, guide, phase, research, vision, session-log, archive
    status      TEXT    DEFAULT 'active',         -- active, accepted, superseded, proposed, deprecated
    phase       TEXT,                             -- 1, 2, 3, all, "1-3"
    audience    TEXT    DEFAULT 'builder',        -- builder, everyone
    content     TEXT    NOT NULL,                 -- full raw markdown content
    word_count  INTEGER DEFAULT 0,
    line_count  INTEGER DEFAULT 0,
    last_updated TEXT,                            -- from frontmatter
    created_at  TEXT    DEFAULT (datetime('now')),
    indexed_at  TEXT    DEFAULT (datetime('now'))
);

-- Major sections (## headings) within each document
CREATE TABLE IF NOT EXISTS sections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    heading     TEXT    NOT NULL,                 -- the ## heading text
    level       INTEGER NOT NULL DEFAULT 2,       -- heading level (1-6)
    content     TEXT    NOT NULL,                 -- section content (until next heading of same or higher level)
    position    INTEGER NOT NULL DEFAULT 0,       -- ordering within document
    word_count  INTEGER DEFAULT 0
);

-- Extracted concepts (technologies, patterns, tools, principles)
CREATE TABLE IF NOT EXISTS concepts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL UNIQUE,
    category    TEXT,                             -- technology, pattern, principle, tool, constraint
    description TEXT,
    phase       TEXT,                             -- which phase this concept belongs to
    status      TEXT    DEFAULT 'active'          -- active, deprecated, proposed
);

-- ADR decisions (structured extraction from decision docs)
CREATE TABLE IF NOT EXISTS decisions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,
    adr_number  INTEGER,                          -- 1, 2, 3, 4, 5...
    title       TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'accepted', -- accepted, superseded, proposed
    date        TEXT,
    context     TEXT,                              -- the Context section
    decision    TEXT,                              -- the Decision section
    consequences_positive TEXT,
    consequences_negative TEXT,
    alternatives TEXT,                             -- alternatives considered
    supersedes  TEXT                               -- path to superseded ADR
);

-- Critique findings (from ralph loop sessions)
CREATE TABLE IF NOT EXISTS findings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,
    session     TEXT,                              -- e.g. "Session 5", "Session 6"
    iteration   INTEGER,                           -- loop iteration number
    severity    TEXT    DEFAULT 'MEDIUM',          -- CRITICAL, HIGH, MEDIUM, LOW
    title       TEXT    NOT NULL,
    description TEXT,
    recommendation TEXT,
    status      TEXT    DEFAULT 'open',            -- open, addressed, deferred, rejected
    addressed_in TEXT                              -- path to file that addresses this
);

-- Risks and unvalidated assumptions
CREATE TABLE IF NOT EXISTS risks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,
    description TEXT    NOT NULL,
    severity    TEXT    DEFAULT 'MEDIUM',          -- CRITICAL, HIGH, MEDIUM, LOW
    status      TEXT    DEFAULT 'open',            -- open, mitigated, validated, accepted
    validation_method TEXT,                        -- how to validate
    mitigation  TEXT,                              -- how it's mitigated
    phase       TEXT                               -- which phase it affects
);

-- Tag taxonomy
CREATE TABLE IF NOT EXISTS tags (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT    NOT NULL UNIQUE
);

-- Many-to-many: documents ↔ tags
CREATE TABLE IF NOT EXISTS document_tags (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    tag_id      INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (document_id, tag_id)
);

-- Many-to-many: concepts ↔ documents (where a concept is discussed)
CREATE TABLE IF NOT EXISTS concept_mentions (
    concept_id  INTEGER NOT NULL REFERENCES concepts(id) ON DELETE CASCADE,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    section_id  INTEGER REFERENCES sections(id) ON DELETE CASCADE,
    context     TEXT,                              -- surrounding text snippet
    PRIMARY KEY (concept_id, document_id, section_id)
);

-- Cross-references between documents
CREATE TABLE IF NOT EXISTS cross_references (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    source_doc_id   INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    target_doc_id   INTEGER REFERENCES documents(id) ON DELETE SET NULL,
    target_path     TEXT    NOT NULL,              -- raw link target (may not resolve)
    link_text       TEXT,                          -- the markdown link text
    context         TEXT                           -- surrounding sentence
);

-- ============================================================================
-- FTS5 Virtual Tables (Full-Text Search)
-- ============================================================================

-- Search across all document content
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    title,
    content,
    category,
    content='documents',
    content_rowid='id',
    tokenize='porter unicode61'
);

-- Search across document sections
CREATE VIRTUAL TABLE IF NOT EXISTS sections_fts USING fts5(
    heading,
    content,
    content='sections',
    content_rowid='id',
    tokenize='porter unicode61'
);

-- Search across concepts
CREATE VIRTUAL TABLE IF NOT EXISTS concepts_fts USING fts5(
    name,
    description,
    category,
    content='concepts',
    content_rowid='id',
    tokenize='porter unicode61'
);

-- Search across decisions
CREATE VIRTUAL TABLE IF NOT EXISTS decisions_fts USING fts5(
    title,
    context,
    decision,
    consequences_positive,
    consequences_negative,
    content='decisions',
    content_rowid='id',
    tokenize='porter unicode61'
);

-- Search across findings
CREATE VIRTUAL TABLE IF NOT EXISTS findings_fts USING fts5(
    title,
    description,
    recommendation,
    content='findings',
    content_rowid='id',
    tokenize='porter unicode61'
);

-- ============================================================================
-- Triggers to keep FTS indexes in sync
-- ============================================================================

CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents BEGIN
    INSERT INTO documents_fts(rowid, title, content, category)
    VALUES (new.id, new.title, new.content, new.category);
END;

CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, content, category)
    VALUES ('delete', old.id, old.title, old.content, old.category);
END;

CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, content, category)
    VALUES ('delete', old.id, old.title, old.content, old.category);
    INSERT INTO documents_fts(rowid, title, content, category)
    VALUES (new.id, new.title, new.content, new.category);
END;

CREATE TRIGGER IF NOT EXISTS sections_ai AFTER INSERT ON sections BEGIN
    INSERT INTO sections_fts(rowid, heading, content)
    VALUES (new.id, new.heading, new.content);
END;

CREATE TRIGGER IF NOT EXISTS sections_ad AFTER DELETE ON sections BEGIN
    INSERT INTO sections_fts(sections_fts, rowid, heading, content)
    VALUES ('delete', old.id, old.heading, old.content);
END;

CREATE TRIGGER IF NOT EXISTS concepts_ai AFTER INSERT ON concepts BEGIN
    INSERT INTO concepts_fts(rowid, name, description, category)
    VALUES (new.id, new.name, new.description, new.category);
END;

CREATE TRIGGER IF NOT EXISTS decisions_ai AFTER INSERT ON decisions BEGIN
    INSERT INTO decisions_fts(rowid, title, context, decision, consequences_positive, consequences_negative)
    VALUES (new.id, new.title, new.context, new.decision, new.consequences_positive, new.consequences_negative);
END;

CREATE TRIGGER IF NOT EXISTS findings_ai AFTER INSERT ON findings BEGIN
    INSERT INTO findings_fts(rowid, title, description, recommendation)
    VALUES (new.id, new.title, new.description, new.recommendation);
END;

-- ============================================================================
-- Convenience Views
-- ============================================================================

-- Note: Unified search must be done via parameterized queries in application
-- code, since SQLite views cannot accept parameters. Use the search()
-- function in query.py or the --query flag of extract.py instead.
-- Example:
--   SELECT d.category, d.title, d.path,
--          snippet(documents_fts, 1, '>>>', '<<<', '...', 40)
--   FROM documents_fts
--   JOIN documents d ON d.id = documents_fts.rowid
--   WHERE documents_fts MATCH ?
--   ORDER BY rank LIMIT 10;

-- Active decisions overview
CREATE VIEW IF NOT EXISTS v_active_decisions AS
    SELECT d.adr_number, d.title, d.status, d.date, doc.path
    FROM decisions d
    LEFT JOIN documents doc ON doc.id = d.document_id
    WHERE d.status = 'accepted'
    ORDER BY d.adr_number;

-- Open risks by severity
CREATE VIEW IF NOT EXISTS v_open_risks AS
    SELECT r.severity, r.description, r.mitigation, r.phase, d.path
    FROM risks r
    LEFT JOIN documents d ON d.id = r.document_id
    WHERE r.status IN ('open', 'mitigated')
    ORDER BY
        CASE r.severity
            WHEN 'CRITICAL' THEN 1
            WHEN 'HIGH' THEN 2
            WHEN 'MEDIUM' THEN 3
            WHEN 'LOW' THEN 4
        END;

-- Documents by phase
CREATE VIEW IF NOT EXISTS v_docs_by_phase AS
    SELECT d.phase, d.category, d.title, d.path, d.status, d.word_count
    FROM documents d
    ORDER BY d.phase, d.category, d.title;

-- Concept index
CREATE VIEW IF NOT EXISTS v_concept_index AS
    SELECT c.name, c.category, c.description, c.phase,
           GROUP_CONCAT(d.path, ', ') AS mentioned_in
    FROM concepts c
    LEFT JOIN concept_mentions cm ON cm.concept_id = c.id
    LEFT JOIN documents d ON d.id = cm.document_id
    GROUP BY c.id
    ORDER BY c.category, c.name;

-- ============================================================================
-- Session Contribution Tracking
-- ============================================================================

-- Tracks items contributed by automation tools (kdb-contribute, kdb-session-end)
-- Created lazily by tools via ensure_contributions_table() in kdb_common.py
CREATE TABLE IF NOT EXISTS session_contributions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session         TEXT    NOT NULL,
    table_name      TEXT    NOT NULL,  -- 'findings', 'risks', 'concepts'
    row_id          INTEGER NOT NULL,
    contributed_at  TEXT    DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_session_contributions_session
    ON session_contributions(session);

-- ============================================================================
-- Indexes for common queries
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_documents_category ON documents(category);
CREATE INDEX IF NOT EXISTS idx_documents_phase ON documents(phase);
CREATE INDEX IF NOT EXISTS idx_documents_status ON documents(status);
CREATE INDEX IF NOT EXISTS idx_sections_document ON sections(document_id);
CREATE INDEX IF NOT EXISTS idx_findings_severity ON findings(severity);
CREATE INDEX IF NOT EXISTS idx_findings_status ON findings(status);
CREATE INDEX IF NOT EXISTS idx_risks_severity ON risks(severity);
CREATE INDEX IF NOT EXISTS idx_risks_status ON risks(status);
CREATE INDEX IF NOT EXISTS idx_decisions_status ON decisions(status);
CREATE INDEX IF NOT EXISTS idx_cross_references_source ON cross_references(source_doc_id);
CREATE INDEX IF NOT EXISTS idx_cross_references_target ON cross_references(target_doc_id);
