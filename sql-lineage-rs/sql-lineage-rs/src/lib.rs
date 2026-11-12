//! Small SQL table-level dependency extractor for educational purposes.
//! This is NOT a full SQL parser and should not be used for production migrations.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Word,
    QuotedIdentifier,
    Literal,
    Symbol,
}

#[derive(Debug, Clone)]
struct Token {
    value: String,
    kind: Kind,
}

impl Token {
    fn keyword(&self, word: &str) -> bool {
        self.kind == Kind::Word && self.value.eq_ignore_ascii_case(word)
    }

    fn symbol(&self, symbol: &str) -> bool {
        self.kind == Kind::Symbol && self.value == symbol
    }

    fn identifier(&self) -> bool {
        matches!(self.kind, Kind::Word | Kind::QuotedIdentifier)
    }
}

/// Tokenize common SQL tokens, ignoring comments and protecting quoted strings.
fn tokenize(sql: &str) -> Result<Vec<Token>, String> {
    let mut iter = sql.chars().peekable();
    let mut result = Vec::new();

    while let Some(c) = iter.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '-' && iter.peek() == Some(&'-') {
            iter.next();
            for x in iter.by_ref() {
                if x == '\n' { break; }
            }
            continue;
        }
        if c == '/' && iter.peek() == Some(&'*') {
            iter.next();
            let mut last = '\0';
            let mut closed = false;
            for x in iter.by_ref() {
                if last == '*' && x == '/' { closed = true; break; }
                last = x;
            }
            if !closed { return Err("Commentaire /* ... */ non terminé".into()); }
            continue;
        }
        if c == '\'' || c == '"' || c == '`' {
            let mut value = String::new();
            let mut closed = false;
            while let Some(x) = iter.next() {
                if x == c {
                    // SQL escapes a quote by repeating it: '' or "".
                    if iter.peek() == Some(&c) {
                        iter.next();
                        value.push(c);
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    value.push(x);
                }
            }
            if !closed { return Err("Chaîne ou identifiant cité non terminé".into()); }
            result.push(Token {
                value,
                kind: if c == '\'' { Kind::Literal } else { Kind::QuotedIdentifier },
            });
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '$' {
            let mut value = c.to_string();
            while let Some(x) = iter.peek() {
                if x.is_alphanumeric() || *x == '_' || *x == '$' {
                    value.push(*x);
                    iter.next();
                } else {
                    break;
                }
            }
            result.push(Token { value, kind: Kind::Word });
            continue;
        }
        result.push(Token { value: c.to_string(), kind: Kind::Symbol });
    }
    Ok(result)
}

fn name_at(tokens: &[Token], index: usize) -> Option<(String, usize)> {
    if index >= tokens.len() || !tokens[index].identifier() {
        return None;
    }
    let mut name = tokens[index].value.to_lowercase();
    let mut next = index + 1;
    while next + 1 < tokens.len()
        && tokens[next].symbol(".")
        && tokens[next + 1].identifier()
    {
        name.push('.');
        name.push_str(&tokens[next + 1].value.to_lowercase());
        next += 2;
    }
    Some((name, next))
}

fn skip_parentheses(tokens: &[Token], begin: usize) -> Result<usize, String> {
    if !tokens.get(begin).is_some_and(|t| t.symbol("(")) {
        return Err("Parenthèse ouvrante attendue".into());
    }
    let mut level = 0;
    for (pos, token) in tokens.iter().enumerate().skip(begin) {
        if token.symbol("(") { level += 1; }
        if token.symbol(")") {
            level -= 1;
            if level == 0 { return Ok(pos + 1); }
        }
    }
    Err("Parenthèses non équilibrées".into())
}

fn starting_ctes(tokens: &[Token]) -> Result<(BTreeSet<String>, usize), String> {
    let mut ctes = BTreeSet::new();
    if tokens.first().is_none_or(|t| !t.keyword("WITH")) {
        return Ok((ctes, 0));
    }
    let mut i = 1;
    if tokens.get(i).is_some_and(|t| t.keyword("RECURSIVE")) { i += 1; }
    loop {
        let (cte, next) = name_at(tokens, i).ok_or("Nom de CTE attendu après WITH")?;
        ctes.insert(cte);
        i = next;
        if tokens.get(i).is_some_and(|t| t.symbol("(")) {
            i = skip_parentheses(tokens, i)?; // optional CTE column list
        }
        if !tokens.get(i).is_some_and(|t| t.keyword("AS")) {
            return Err("AS attendu dans la déclaration de CTE".into());
        }
        i += 1;
        if tokens.get(i).is_some_and(|t| t.keyword("NOT")) { i += 1; }
        if tokens.get(i).is_some_and(|t| t.keyword("MATERIALIZED")) { i += 1; }
        i = skip_parentheses(tokens, i)?;
        if tokens.get(i).is_some_and(|t| t.symbol(",")) {
            i += 1;
        } else {
            break;
        }
    }
    Ok((ctes, i))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub kind: String,
    pub target: Option<String>,
    pub sources: Vec<String>,
}

fn analyze_one(tokens: &[Token]) -> Result<Statement, String> {
    let (ctes, start) = starting_ctes(tokens)?;
    let head = tokens.get(start).ok_or("Instruction SQL absente")?;
    let mut target = None;
    let kind: &str;

    if head.keyword("SELECT") {
        kind = "select";
    } else if head.keyword("INSERT") {
        kind = "insert";
        let mut i = start + 1;
        if tokens.get(i).is_some_and(|t| t.keyword("INTO")) { i += 1; }
        let (name, _) = name_at(tokens, i).ok_or("Table cible attendue après INSERT")?;
        target = Some(name);
        if !tokens.iter().any(|t| t.keyword("SELECT")) {
            return Err("Seul INSERT ... SELECT est pris en charge".into());
        }
    } else if head.keyword("CREATE") {
        let mut i = start + 1;
        if tokens.get(i).is_some_and(|t| t.keyword("OR")) &&
           tokens.get(i + 1).is_some_and(|t| t.keyword("REPLACE")) { i += 2; }
        if tokens.get(i).is_some_and(|t| t.keyword("TEMP") || t.keyword("TEMPORARY")) { i += 1; }
        let obj_type = tokens.get(i).ok_or("Type d'objet absent après CREATE")?;
        if obj_type.keyword("VIEW") { kind = "create_view"; }
        else if obj_type.keyword("TABLE") { kind = "create_table_as"; }
        else { return Err("Seuls CREATE VIEW et CREATE TABLE ... AS SELECT sont pris en charge".into()); }
        let (name, _) = name_at(tokens, i + 1).ok_or("Nom de la cible attendu après CREATE")?;
        target = Some(name);
        if !tokens.iter().any(|t| t.keyword("AS")) ||
           !tokens.iter().any(|t| t.keyword("SELECT")) {
            return Err("Seuls CREATE ... AS SELECT sont pris en charge".into());
        }
    } else {
        return Err(format!("Instruction SQL non prise en charge : {}", head.value));
    }

    // The accepted scope is table-level dependencies after FROM and JOIN.
    // Nested SELECTs are scanned too; aliases and column dependencies are not inferred.
    let mut sources = BTreeSet::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].keyword("FROM") || tokens[i].keyword("JOIN") {
            if let Some((name, next)) = name_at(tokens, i + 1) {
                if !ctes.contains(&name) { sources.insert(name); }
                i = next;
                continue;
            }
        }
        i += 1;
    }
    if let Some(t) = &target {
        sources.remove(t); // avoid misleading self-dependencies in this simple prototype
    }
    Ok(Statement { kind: kind.into(), target, sources: sources.into_iter().collect() })
}

/// Analyze SELECT, INSERT...SELECT, CREATE VIEW AS SELECT and CREATE TABLE AS SELECT.
pub fn analyze(sql: &str) -> Result<Vec<Statement>, String> {
    let tokens = tokenize(sql)?;
    let mut statements = Vec::new();
    let mut start = 0;
    for end in 0..=tokens.len() {
        if end == tokens.len() || tokens[end].symbol(";") {
            if start < end {
                let ordinal = statements.len() + 1;
                let parsed = analyze_one(&tokens[start..end])
                    .map_err(|e| format!("Instruction {ordinal} : {e}"))?;
                statements.push(parsed);
            }
            start = end + 1;
        }
    }
    Ok(statements)
}

fn json_escape(text: &str) -> String {
    let mut escaped = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

/// Emit JSON with only the supported table-level metadata.
pub fn json_output(statements: &[Statement]) -> String {
    let mut parts = Vec::new();
    for item in statements {
        let sources: Vec<String> = item.sources.iter().map(|s| json_escape(s)).collect();
        parts.push(format!(
            "{{\"kind\":{},\"target\":{},\"sources\":[{}]}}",
            json_escape(&item.kind),
            item.target.as_ref().map_or("null".into(), |t| json_escape(t)),
            sources.join(",")
        ));
    }
    format!("{{\"statements\":[{}]}}", parts.join(","))
}

/// Emit a Graphviz DOT graph (source tables -> target tables).
pub fn dot_output(statements: &[Statement]) -> String {
    let mut edges = BTreeSet::new();
    let mut nodes = BTreeSet::new();
    for item in statements {
        if let Some(target) = &item.target {
            nodes.insert(target.clone());
            for src in &item.sources {
                edges.insert((src.clone(), target.clone()));
                nodes.insert(src.clone());
            }
        } else {
            nodes.extend(item.sources.iter().cloned());
        }
    }
    let mut out = String::from("digraph lineage {\n  rankdir=LR;\n  node [shape=box];\n");
    for name in nodes { out.push_str(&format!("  {};\n", json_escape(&name))); }
    for (src, dst) in edges {
        out.push_str(&format!("  {} -> {};\n", json_escape(&src), json_escape(&dst)));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_and_schema_names() {
        let data = analyze("SELECT * FROM Sales.Orders o JOIN crm.Customers c ON c.id=o.id;").unwrap();
        assert_eq!(data[0].sources, vec!["crm.customers", "sales.orders"]);
        assert_eq!(data[0].target, None);
    }

    #[test]
    fn create_view_dependency() {
        let data = analyze("CREATE VIEW reports.revenue AS SELECT * FROM sales.orders JOIN crm.clients ON 1=1").unwrap();
        assert_eq!(data[0].target.as_deref(), Some("reports.revenue"));
        assert!(dot_output(&data).contains("\"sales.orders\" -> \"reports.revenue\""));
    }

    #[test]
    fn ctes_do_not_become_source_tables() {
        let data = analyze("WITH recent AS (SELECT * FROM orders) SELECT * FROM recent JOIN people ON 1=1;").unwrap();
        assert_eq!(data[0].sources, vec!["orders", "people"]);
    }

    #[test]
    fn insert_with_cte() {
        let data = analyze("WITH recent AS (SELECT * FROM raw.orders) INSERT INTO mart.orders SELECT * FROM recent").unwrap();
        assert_eq!(data[0].target.as_deref(), Some("mart.orders"));
        assert_eq!(data[0].sources, vec!["raw.orders"]);
    }

    #[test]
    fn quote_and_comment_protection() {
        let data = analyze("-- FROM fake\n SELECT 'FROM evil JOIN nope' AS s FROM \"Sales\".\"Orders\" /* JOIN hack */;").unwrap();
        assert_eq!(data[0].sources, vec!["sales.orders"]);
    }

    #[test]
    fn multiple_statements_and_semi_in_string() {
        let data = analyze("SELECT ';' FROM a; SELECT 3 FROM b;").unwrap();
        assert_eq!(data.len(), 2);
    }

    #[test]
    fn unsupported_statement_is_explicit() {
        assert!(analyze("DROP TABLE test;").is_err());
    }

    #[test]
    fn unfinished_quotes_are_rejected() {
        assert!(analyze("SELECT 'abc FROM orders").is_err());
    }

    #[test]
    fn json_is_well_shaped() {
        let data = analyze("CREATE TABLE summary AS SELECT * FROM raw.orders;").unwrap();
        assert!(json_output(&data).contains("\"sources\":[\"raw.orders\"]"));
    }
}
