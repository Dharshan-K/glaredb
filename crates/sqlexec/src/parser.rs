pub mod options;

use crate::errors::Result;
use datafusion::sql::sqlparser::ast::{self, Ident, ObjectName};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::keywords::Keyword;
use datafusion::sql::sqlparser::parser::{Parser, ParserError};
use datafusion::sql::sqlparser::tokenizer::{Token, Tokenizer, Word};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fmt;

use self::options::{OptionValue, StmtOptions};

/// Wrapper around our custom parse for parsing a sql statement.
pub fn parse_sql(sql: &str) -> Result<VecDeque<StatementWithExtensions>> {
    let stmts = CustomParser::parse_sql(sql)?;
    Ok(stmts)
}

/// DDL extension for GlareDB's external tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateExternalTableStmt {
    /// Name of the table.
    pub name: ObjectName,
    /// replace if it exists
    pub or_replace: bool,
    /// Optionally don't error if table exists.
    pub if_not_exists: bool,
    /// Data source type.
    pub datasource: Ident,
    /// Optional tunnel to use for connection.
    pub tunnel: Option<Ident>,
    /// Credentials to use for configuration.
    pub credentials: Option<Ident>,
    /// Datasource specific options.
    pub options: StmtOptions,
}

impl fmt::Display for CreateExternalTableStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CREATE {or_replace}EXTERNAL TABLE {if_not_exists}{name} FROM {datasource}{tunnel}{creds}",
            or_replace = if self.or_replace { "OR REPLACE " } else { "" },
            if_not_exists = if self.if_not_exists { "IF NOT EXISTS " } else { "" },
            name = self.name,
            datasource = self.datasource,
            tunnel = self.tunnel.as_ref().map(|t| format!(" TUNNEL {}", t)).unwrap_or_default(),
            creds = self.credentials.as_ref().map(|c| format!(" CREDENTIALS {}", c)).unwrap_or_default(),
        )?;

        if !self.options.is_empty() {
            write!(f, " {}", self.options)?;
        }

        Ok(())
    }
}

/// DDL for external databases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateExternalDatabaseStmt {
    /// Name of the database as it exists in GlareDB.
    pub name: Ident,
    /// replace if it exists
    pub or_replace: bool,
    /// Optionally don't error if database exists.
    pub if_not_exists: bool,
    /// The data source type the connection is for.
    pub datasource: Ident,
    /// Optional tunnel to use for connection.
    pub tunnel: Option<Ident>,
    /// Credentials to use for configuration.
    pub credentials: Option<Ident>,
    /// Datasource specific options.
    pub options: StmtOptions,
}

impl fmt::Display for CreateExternalDatabaseStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CREATE {or_replace}EXTERNAL DATABASE {if_not_exists}{name} FROM {datasource}{tunnel}{creds}",
            or_replace = if self.or_replace { "OR REPLACE " } else { "" },
            if_not_exists = if self.if_not_exists { "IF NOT EXISTS " } else { "" },
            name = self.name,
            datasource = self.datasource,
            tunnel = self.tunnel.as_ref().map(|t| format!(" TUNNEL {}", t)).unwrap_or_default(),
            creds = self.credentials.as_ref().map(|c| format!(" CREDENTIALS {}", c)).unwrap_or_default(),
        )?;

        if !self.options.is_empty() {
            write!(f, " {}", self.options)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropDatabaseStmt {
    pub names: Vec<Ident>,
    pub if_exists: bool,
}

impl fmt::Display for DropDatabaseStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DROP DATABASE ")?;
        if self.if_exists {
            write!(f, "IF EXISTS ")?;
        }
        let mut sep = "";
        for name in self.names.iter() {
            write!(f, "{sep}{name}")?;
            sep = ", ";
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlterDatabaseRenameStmt {
    pub name: Ident,
    pub new_name: Ident,
}

impl fmt::Display for AlterDatabaseRenameStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ALTER DATABASE ")?;
        write!(f, "{}", self.name)?;
        write!(f, " RENAME TO ")?;
        write!(f, "{}", self.new_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTunnelStmt {
    /// Name of the tunnel as it exists in GlareDB.
    pub name: Ident,
    /// Optionally don't error if tunnel exists.
    pub if_not_exists: bool,
    /// The tunnel type the connection is for.
    pub tunnel: Ident,
    /// Tunnel specific options.
    pub options: StmtOptions,
}

impl fmt::Display for CreateTunnelStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CREATE TUNNEL")?;
        if self.if_not_exists {
            write!(f, " IF NOT EXISTS")?;
        }
        write!(f, " {} FROM {}", self.name, self.tunnel)?;
        if !self.options.is_empty() {
            write!(f, " {}", self.options)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTunnelStmt {
    pub names: Vec<Ident>,
    pub if_exists: bool,
}

impl fmt::Display for DropTunnelStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DROP TUNNEL ")?;
        if self.if_exists {
            write!(f, "IF EXISTS ")?;
        }
        let mut sep = "";
        for name in self.names.iter() {
            write!(f, "{sep}{name}")?;
            sep = ", ";
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlterTunnelAction {
    RotateKeys,
}

impl fmt::Display for AlterTunnelAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RotateKeys => f.write_str("ROTATE KEYS"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlterTunnelStmt {
    pub name: Ident,
    pub if_exists: bool,
    pub action: AlterTunnelAction,
}

impl fmt::Display for AlterTunnelStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ALTER TUNNEL ")?;
        if self.if_exists {
            write!(f, "IF EXISTS ")?;
        }
        write!(f, "{} {}", self.name, self.action)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCredentialsStmt {
    /// Name of the credentials as it exists in GlareDB.
    pub name: Ident,
    /// The credentials provider.
    pub provider: Ident,
    /// Credentials specific options.
    pub options: StmtOptions,
    /// Optional comment (what the credentials are for).
    pub comment: String,
}

impl fmt::Display for CreateCredentialsStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CREATE CREDENTIALS {} PROVIDER {}",
            self.name, self.provider
        )?;
        if !self.options.is_empty() {
            write!(f, " {}", self.options)?;
        }

        if !self.comment.is_empty() {
            write!(f, " COMMENT '{}'", self.comment)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropCredentialsStmt {
    pub names: Vec<Ident>,
    pub if_exists: bool,
}

impl fmt::Display for DropCredentialsStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DROP CREDENTIALS ")?;
        if self.if_exists {
            write!(f, "IF EXISTS ")?;
        }
        let mut sep = "";
        for name in self.names.iter() {
            write!(f, "{sep}{name}")?;
            sep = ", ";
        }
        Ok(())
    }
}

/// A source for a COPY TO statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyToSource {
    Table(ObjectName),
    Query(ast::Query),
}

impl fmt::Display for CopyToSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyToSource::Table(table) => write!(f, "{table}"),
            CopyToSource::Query(query) => write!(f, "({query})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyToStmt {
    /// Source to copy the data from (table or query).
    pub source: CopyToSource,
    /// Destination to copy the data to.
    pub dest: Ident,
    /// Optional format (in which to copy the data in).
    pub format: Option<Ident>,
    /// Optional credentials (for cloud storage).
    pub credentials: Option<Ident>,
    /// COPY TO specific options.
    pub options: StmtOptions,
}

impl fmt::Display for CopyToStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "COPY {} TO {}", self.source, self.dest)?;
        if let Some(format) = self.format.as_ref() {
            write!(f, " FORMAT {format}")?;
        }
        if let Some(creds) = self.credentials.as_ref() {
            write!(f, " CREDENTIALS {creds}")?;
        }
        if !self.options.is_empty() {
            write!(f, " {}", self.options)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementWithExtensions {
    /// Statement parsed by `sqlparser`.
    Statement(ast::Statement),
    /// Create external table extension.
    CreateExternalTable(CreateExternalTableStmt),
    /// Create external database extension.
    CreateExternalDatabase(CreateExternalDatabaseStmt),
    /// Drop database extension.
    DropDatabase(DropDatabaseStmt),
    // Alter database extension (rename).
    AlterDatabaseRename(AlterDatabaseRenameStmt),
    /// Create tunnel extension.
    CreateTunnel(CreateTunnelStmt),
    /// Drop tunnel extension.
    DropTunnel(DropTunnelStmt),
    /// Alter tunnel extension.
    AlterTunnel(AlterTunnelStmt),
    /// Create credentials extension.
    CreateCredentials(CreateCredentialsStmt),
    /// Drop credentials extension.
    DropCredentials(DropCredentialsStmt),
    /// Copy To extension.
    CopyTo(CopyToStmt),
}

impl fmt::Display for StatementWithExtensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatementWithExtensions::Statement(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::CreateExternalTable(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::CreateExternalDatabase(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::DropDatabase(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::AlterDatabaseRename(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::CreateTunnel(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::DropTunnel(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::AlterTunnel(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::CreateCredentials(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::DropCredentials(stmt) => write!(f, "{}", stmt),
            StatementWithExtensions::CopyTo(stmt) => write!(f, "{}", stmt),
        }
    }
}

/// Parser with our extensions.
pub struct CustomParser<'a> {
    parser: Parser<'a>,
}

impl<'a> CustomParser<'a> {
    pub fn parse_sql(sql: &str) -> Result<VecDeque<StatementWithExtensions>, ParserError> {
        let dialect = GenericDialect {};
        let tokens = Tokenizer::new(&dialect, sql).tokenize()?;
        let mut parser = CustomParser {
            parser: Parser::new(&dialect).with_tokens(tokens),
        };

        let mut stmts = VecDeque::new();
        let mut expecting_statement_delimiter = false;
        loop {
            // ignore empty statements (between successive statement delimiters)
            while parser.parser.consume_token(&Token::SemiColon) {
                expecting_statement_delimiter = false;
            }

            if parser.parser.peek_token() == Token::EOF {
                break;
            }
            if expecting_statement_delimiter {
                return parser.expected("end of statement", parser.parser.peek_token().token);
            }

            let statement = parser.parse_statement()?;
            stmts.push_back(statement);
            expecting_statement_delimiter = true;
        }

        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<StatementWithExtensions, ParserError> {
        match self.parser.peek_token().token {
            Token::Word(w) => match w.keyword {
                Keyword::CREATE => {
                    self.parser.next_token();
                    self.parse_create()
                }
                Keyword::DROP => {
                    self.parser.next_token();
                    self.parse_drop()
                }
                Keyword::ALTER => {
                    self.parser.next_token();
                    self.parse_alter()
                }
                Keyword::COPY => {
                    self.parser.next_token();
                    self.parse_copy()
                }
                _ => Ok(StatementWithExtensions::Statement(
                    self.parser.parse_statement()?,
                )),
            },
            _ => Ok(StatementWithExtensions::Statement(
                self.parser.parse_statement()?,
            )),
        }
    }

    /// Parse a SQL CREATE statement
    fn parse_create(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let or_replace = self.parser.parse_keywords(&[Keyword::OR, Keyword::REPLACE]);

        if self.parser.parse_keyword(Keyword::EXTERNAL) {
            // CREATE EXTERNAL ...
            if self.parser.parse_keyword(Keyword::TABLE) {
                self.parse_create_external_table(or_replace)
            } else if self.parser.parse_keyword(Keyword::DATABASE) {
                self.parse_create_external_database(or_replace)
            } else {
                let next = self.parser.peek_token().token;
                Err(ParserError::ParserError(format!(
                    "Expected 'CREATE EXTERNAL DATABASE' or 'CREATE EXTERNAL TABLE', found 'CREATE EXTERNAL {}'",
                    next
                )))
            }
        } else if self.consume_token(&Token::make_keyword("TUNNEL")) {
            // CREATE TUNNEL ...
            self.parse_create_tunnel()
        } else if self.parser.parse_keyword(Keyword::CREDENTIALS) {
            // CREATE CREDENTIALS ...
            self.parse_create_credentials()
        } else {
            // Fall back to underlying parser.

            if or_replace {
                // backtrack to include OR REPLACE in the statement passed to the underlying parser
                self.parser.prev_token();
                self.parser.prev_token();
            }

            Ok(StatementWithExtensions::Statement(
                self.parser.parse_create()?,
            ))
        }
    }

    /// Parse a SQL ALTER statement
    fn parse_alter(&mut self) -> Result<StatementWithExtensions, ParserError> {
        if self.parser.parse_keyword(Keyword::DATABASE) {
            // ALTER DATABASE ...
            self.parse_alter_database()
        } else if self.consume_token(&Token::make_keyword("TUNNEL")) {
            // ALTER TUNNEL ...
            self.parse_alter_tunnel()
        } else {
            // Fall back to underlying parser.
            Ok(StatementWithExtensions::Statement(
                self.parser.parse_alter()?,
            ))
        }
    }

    /// Parse a COPY statement.
    fn parse_copy(&mut self) -> Result<StatementWithExtensions, ParserError> {
        // Parse table or query source:
        //     COPY table ..
        //     or
        //     COPY (SELECT ..) ..
        let source = if self.parser.consume_token(&Token::LParen) {
            let query = self.parser.parse_query()?;
            self.parser.expect_token(&Token::RParen)?;
            CopyToSource::Query(query)
        } else {
            let table_name = self.parser.parse_object_name()?;
            CopyToSource::Table(table_name)
        };

        // TO 'source'
        self.parser.expect_keyword(Keyword::TO)?;
        let dest = self.parser.parse_identifier()?;

        // [FORMAT ..]
        let format = self.parse_data_format()?;

        // [CREDENTIALS ..]
        let credentials = self.parse_connection_credentials()?;

        // OPTIONS (..)
        let options = self.parse_options()?;

        Ok(StatementWithExtensions::CopyTo(CopyToStmt {
            source,
            dest,
            format,
            credentials,
            options,
        }))
    }

    /// Report unexpected token.
    fn expected<T>(&self, expected: &str, found: Token) -> Result<T, ParserError> {
        Err(ParserError::ParserError(format!(
            "Expected {}, found: {}",
            expected, found
        )))
    }

    fn parse_create_external_table(
        &mut self,
        or_replace: bool,
    ) -> Result<StatementWithExtensions, ParserError> {
        let if_not_exists =
            self.parser
                .parse_keywords(&[Keyword::IF, Keyword::NOT, Keyword::EXISTS]);
        let name = self.parser.parse_object_name()?;
        validate_object_name(&name)?;

        // FROM datasource
        self.parser.expect_keyword(Keyword::FROM)?;
        let datasource = self.parse_object_type("datasource")?;

        // [TUNNEL ...]
        let tunnel = self.parse_connection_tunnel()?;

        // [CREDENTIALS ...]
        let credentials = self.parse_connection_credentials()?;

        // OPTIONS (..)
        let options = self.parse_options()?;

        Ok(StatementWithExtensions::CreateExternalTable(
            CreateExternalTableStmt {
                name,
                or_replace,
                if_not_exists,
                datasource,
                tunnel,
                credentials,
                options,
            },
        ))
    }

    fn parse_create_external_database(
        &mut self,
        or_replace: bool,
    ) -> Result<StatementWithExtensions, ParserError> {
        let if_not_exists =
            self.parser
                .parse_keywords(&[Keyword::IF, Keyword::NOT, Keyword::EXISTS]);

        let name = self.parser.parse_identifier()?;
        validate_ident(&name)?;

        // FROM datasource
        self.parser.expect_keyword(Keyword::FROM)?;
        let datasource = self.parse_object_type("datasource")?;

        // [TUNNEL ...]
        let tunnel = self.parse_connection_tunnel()?;

        // [CREDENTIALS ...]
        let credentials = self.parse_connection_credentials()?;

        // OPTIONS (..)
        let options = self.parse_options()?;

        Ok(StatementWithExtensions::CreateExternalDatabase(
            CreateExternalDatabaseStmt {
                name,
                if_not_exists,
                or_replace,
                datasource,
                tunnel,
                credentials,
                options,
            },
        ))
    }

    fn parse_create_tunnel(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let if_not_exists =
            self.parser
                .parse_keywords(&[Keyword::IF, Keyword::NOT, Keyword::EXISTS]);

        let name = self.parser.parse_identifier()?;
        validate_ident(&name)?;

        // FROM tunnel
        self.parser.expect_keyword(Keyword::FROM)?;
        let tunnel = self.parse_object_type("tunnel")?;

        // OPTIONS (..)
        let options = self.parse_options()?;

        Ok(StatementWithExtensions::CreateTunnel(CreateTunnelStmt {
            name,
            if_not_exists,
            tunnel,
            options,
        }))
    }

    fn parse_create_credentials(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let name = self.parser.parse_identifier()?;
        validate_ident(&name)?;

        // PROVIDER credentials
        self.expect_token(&Token::make_keyword("PROVIDER"))?;
        let provider = self.parse_object_type("credentials")?;

        // OPTIONS (..)
        let options = self.parse_options()?;

        let comment = if self.parser.parse_keyword(Keyword::COMMENT) {
            self.parser.parse_literal_string()?
        } else {
            "".to_owned()
        };

        Ok(StatementWithExtensions::CreateCredentials(
            CreateCredentialsStmt {
                name,
                provider,
                options,
                comment,
            },
        ))
    }

    fn parse_object_type(&mut self, object_type: &str) -> Result<Ident, ParserError> {
        match self.parser.next_token().token {
            Token::Word(w) => Ok(w.to_ident()),
            other => self.expected(object_type, other),
        }
    }

    /// Parse reference to another object (optionally).
    ///
    /// Example: `TUNNEL xyz`...
    fn parse_optional_ref(&mut self, k: &str) -> Result<Option<Ident>, ParserError> {
        let opt = if self.consume_token(&Token::make_keyword(k)) {
            let opt = self.parser.parse_identifier()?;
            validate_ident(&opt)?;
            Some(opt)
        } else {
            None
        };
        Ok(opt)
    }

    fn parse_connection_tunnel(&mut self) -> Result<Option<Ident>, ParserError> {
        self.parse_optional_ref("TUNNEL")
    }

    fn parse_connection_credentials(&mut self) -> Result<Option<Ident>, ParserError> {
        self.parse_optional_ref("CREDENTIALS")
    }

    fn parse_data_format(&mut self) -> Result<Option<Ident>, ParserError> {
        self.parse_optional_ref("FORMAT")
    }

    /// Parse options for a datasource.
    fn parse_options(&mut self) -> Result<StmtOptions, ParserError> {
        // Optional "OPTIONS" keyword
        let _ = self.consume_token(&Token::make_keyword("OPTIONS"));

        if !self.parser.consume_token(&Token::LParen) {
            // If there's no "(" assume there's no options.
            return Ok(StmtOptions::new(BTreeMap::new()));
        }

        let mut options = BTreeMap::new();
        loop {
            if self.parser.consume_token(&Token::RParen) {
                // Break if there are no options `()`.
                break;
            }

            // TODO: Keep the options "key" as identifier so later we can
            // normalize it.
            let key = self.parser.parse_identifier()?.value;

            // Optional `=`
            let _ = self.parser.consume_token(&Token::Eq);

            let value = self.parse_options_value()?;

            options.insert(key, value);
            let comma = self.parser.consume_token(&Token::Comma);

            if self.parser.consume_token(&Token::RParen) {
                // Allow a trailing comma, even though it's not in standard.
                break;
            } else if !comma {
                return self.expected(
                    "',' or ')' after option definition",
                    self.parser.peek_token().token,
                );
            }
        }

        Ok(StmtOptions::new(options))
    }

    fn parse_options_value(&mut self) -> Result<OptionValue, ParserError> {
        let opt_val = if self.consume_token(&Token::make_keyword("SECRET")) {
            OptionValue::Secret(self.parser.parse_identifier()?.value)
        } else {
            let tok = self.parser.next_token();
            match tok.token {
                Token::Word(Word {
                    keyword: Keyword::TRUE,
                    ..
                }) => OptionValue::Boolean(true),
                Token::Word(Word {
                    keyword: Keyword::FALSE,
                    ..
                }) => OptionValue::Boolean(false),
                Token::Word(Word { value, .. }) => OptionValue::UnquotedLiteral(value),
                Token::SingleQuotedString(s) | Token::DoubleQuotedString(s) => {
                    OptionValue::QuotedLiteral(s)
                }
                Token::Number(n, _) => OptionValue::Number(n),
                _ => {
                    return Err(ParserError::ParserError(format!(
                        "Expected string, number or bool, found: {tok}"
                    )))
                }
            }
        };
        Ok(opt_val)
    }

    /// Consume a token return, returning whether or not it was consumed.
    fn consume_token(&mut self, expected: &Token) -> bool {
        let token = self.parser.peek_token().to_string().to_uppercase();
        let token = Token::make_keyword(&token);
        if token == *expected {
            let _ = self.parser.next_token();
            true
        } else {
            false
        }
    }

    fn expect_token(&mut self, expected: &Token) -> Result<(), ParserError> {
        if self.consume_token(expected) {
            Ok(())
        } else {
            self.expected(&expected.to_string(), self.parser.peek_token().token)
        }
    }

    /// Parse a SQL DROP statement
    fn parse_drop(&mut self) -> Result<StatementWithExtensions, ParserError> {
        if self.parser.parse_keyword(Keyword::DATABASE) {
            // DROP DATABASE ...
            self.parse_drop_database()
        } else if self.consume_token(&Token::make_keyword("TUNNEL")) {
            // DROP TUNNEL ...
            self.parse_drop_tunnel()
        } else if self.consume_token(&Token::make_keyword("CREDENTIALS")) {
            // DROP CREDENTIALS ...
            self.parse_drop_credentials()
        } else {
            // Fall back to underlying parser.
            Ok(StatementWithExtensions::Statement(
                self.parser.parse_drop()?,
            ))
        }
    }

    fn parse_drop_database(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let if_exists = self.parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);

        let names = self
            .parser
            .parse_comma_separated(Parser::parse_identifier)?;

        for name in names.iter() {
            validate_ident(name)?;
        }

        Ok(StatementWithExtensions::DropDatabase(DropDatabaseStmt {
            names,
            if_exists,
        }))
    }

    fn parse_drop_tunnel(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let if_exists = self.parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);

        let names = self
            .parser
            .parse_comma_separated(Parser::parse_identifier)?;

        for name in names.iter() {
            validate_ident(name)?;
        }

        Ok(StatementWithExtensions::DropTunnel(DropTunnelStmt {
            names,
            if_exists,
        }))
    }

    fn parse_drop_credentials(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let if_exists = self.parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);

        let names = self
            .parser
            .parse_comma_separated(Parser::parse_identifier)?;

        for name in names.iter() {
            validate_ident(name)?;
        }

        Ok(StatementWithExtensions::DropCredentials(
            DropCredentialsStmt { names, if_exists },
        ))
    }

    fn parse_alter_database(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let name = self.parser.parse_identifier()?;
        validate_ident(&name)?;

        if !self.parser.parse_keywords(&[Keyword::RENAME, Keyword::TO]) {
            return self.expected("RENAME TO", self.parser.peek_token().token);
        }

        let new_name = self.parser.parse_identifier()?;
        validate_ident(&new_name)?;

        Ok(StatementWithExtensions::AlterDatabaseRename(
            AlterDatabaseRenameStmt { name, new_name },
        ))
    }

    fn parse_alter_tunnel(&mut self) -> Result<StatementWithExtensions, ParserError> {
        let if_exists = self.parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);

        let name = self.parser.parse_identifier()?;
        validate_ident(&name)?;

        let mut action = None;

        if self.consume_token(&Token::make_keyword("ROTATE"))
            && self.consume_token(&Token::make_keyword("KEYS"))
        {
            action = Some(AlterTunnelAction::RotateKeys);
        }

        if let Some(action) = action {
            Ok(StatementWithExtensions::AlterTunnel(AlterTunnelStmt {
                name,
                if_exists,
                action,
            }))
        } else {
            let next_token = self.parser.next_token();
            self.expected("a valid alter tunnel action", next_token.token)
        }
    }
}

pub fn validate_ident(ident: &ast::Ident) -> Result<(), ParserError> {
    sqlbuiltins::validation::validate_object_name(&ident.value)
        .map_err(|e| ParserError::ParserError(e.to_string()))
}

/// Validate object names a `Vec<ast::Idents>`
pub fn validate_object_name(name: &ast::ObjectName) -> Result<(), ParserError> {
    for ident in name.0.iter() {
        validate_ident(ident)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_table_display() {
        let mut options = BTreeMap::new();
        options.insert(
            "postgres_conn".to_string(),
            OptionValue::QuotedLiteral("host=localhost user=postgres".to_string()),
        );
        options.insert(
            "schema".to_string(),
            OptionValue::QuotedLiteral("public".to_string()),
        );
        options.insert(
            "table".to_string(),
            OptionValue::Secret("pg_table".to_string()),
        );
        let mut stmt = CreateExternalTableStmt {
            name: ObjectName(vec![Ident::new("test")]),
            or_replace: true,
            if_not_exists: false,
            datasource: Ident::new("postgres"),
            tunnel: None,
            credentials: None,
            options: StmtOptions::new(options),
        };
        println!("{:?}", stmt);

        let out = stmt.to_string();
        assert_eq!("CREATE OR REPLACE EXTERNAL TABLE test FROM postgres OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public', table = SECRET pg_table)", out);

        stmt.tunnel = Some(Ident::new("ssh_tunnel"));
        let out = stmt.to_string();
        assert_eq!("CREATE OR REPLACE EXTERNAL TABLE test FROM postgres TUNNEL ssh_tunnel OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public', table = SECRET pg_table)", out);
    }

    #[test]
    fn external_table_parse() {
        let sql = "CREATE EXTERNAL TABLE test FROM postgres OPTIONS (postgres_conn = 'host=localhost user=postgres', schema='public', table=secret pg_table)";
        let mut stmts = CustomParser::parse_sql(sql).unwrap();

        let stmt = stmts.pop_front().unwrap();
        let mut options = BTreeMap::new();
        options.insert(
            "postgres_conn".to_string(),
            OptionValue::QuotedLiteral("host=localhost user=postgres".to_string()),
        );
        options.insert(
            "schema".to_string(),
            OptionValue::QuotedLiteral("public".to_string()),
        );
        options.insert(
            "table".to_string(),
            OptionValue::Secret("pg_table".to_string()),
        );

        let mut parsed_stmt = CreateExternalTableStmt {
            name: ObjectName(vec![Ident::new("test")]),
            or_replace: false,
            if_not_exists: false,
            datasource: Ident::new("postgres"),
            tunnel: None,
            credentials: None,
            options: StmtOptions::new(options),
        };

        assert_eq!(
            StatementWithExtensions::CreateExternalTable(parsed_stmt.clone()),
            stmt
        );

        let sql = "CREATE EXTERNAL TABLE test FROM postgres TUNNEL ssh_tunnel OPTIONS (postgres_conn = 'host=localhost user=postgres', schema='public', table=secret pg_table)";
        let mut stmts = CustomParser::parse_sql(sql).unwrap();

        let stmt = stmts.pop_front().unwrap();
        parsed_stmt.tunnel = Some(Ident::new("ssh_tunnel"));
        assert_eq!(
            StatementWithExtensions::CreateExternalTable(parsed_stmt),
            stmt
        );
    }

    #[test]
    fn create_external_table_roundtrips() {
        let test_cases = [
            "CREATE EXTERNAL TABLE test FROM postgres OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE IF NOT EXISTS test FROM postgres OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE test FROM postgres TUNNEL my_ssh OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE IF NOT EXISTS test FROM postgres TUNNEL my_ssh OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE test FROM postgres CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE IF NOT EXISTS test FROM postgres CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE test FROM postgres TUNNEL my_ssh CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE OR REPLACE EXTERNAL TABLE test FROM postgres TUNNEL my_ssh CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
            "CREATE EXTERNAL TABLE IF NOT EXISTS test FROM postgres TUNNEL my_ssh CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres', schema = 'public')",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn create_external_database_roundtrips() {
        let test_cases = [
            "CREATE EXTERNAL DATABASE qa FROM postgres OPTIONS (host = 'localhost', user = 'user')",
            "CREATE EXTERNAL DATABASE IF NOT EXISTS qa FROM postgres OPTIONS (host = 'localhost', user = 'user')",
            "CREATE EXTERNAL DATABASE qa FROM postgres TUNNEL my_ssh OPTIONS (host = 'localhost', user = 'user')",
            "CREATE OR REPLACE EXTERNAL DATABASE qa FROM postgres TUNNEL my_ssh OPTIONS (host = 'localhost', user = 'user')",
            "CREATE EXTERNAL DATABASE IF NOT EXISTS qa FROM postgres TUNNEL my_ssh OPTIONS (host = 'localhost', user = 'user')",
            "CREATE EXTERNAL DATABASE test FROM postgres CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres')",
            "CREATE EXTERNAL DATABASE IF NOT EXISTS test FROM postgres CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres')",
            "CREATE EXTERNAL DATABASE test FROM postgres TUNNEL my_ssh CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres')",
            "CREATE OR REPLACE EXTERNAL DATABASE test FROM postgres TUNNEL my_ssh CREDENTIALS my_pg OPTIONS (postgres_conn = 'host=localhost user=postgres')",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn create_tunnel_roundtrips() {
        let test_cases = [
            "CREATE TUNNEL qa FROM postgres OPTIONS (host = 'localhost', user = 'user')",
            "CREATE TUNNEL IF NOT EXISTS qa FROM postgres OPTIONS (host = 'localhost', user = 'user')",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn create_credentials_roundtrips() {
        let test_cases = [
            "CREATE CREDENTIALS qa PROVIDER debug OPTIONS (table_type = 'never_ending')",
            "CREATE CREDENTIALS qa PROVIDER debug OPTIONS (table_type = 'never_ending') COMMENT 'for debug'",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn drop_database_roundtrips() {
        let test_cases = ["DROP DATABASE my_db", "DROP DATABASE IF EXISTS my_db"];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn drop_tunnel_roundtrips() {
        let test_cases = ["DROP TUNNEL my_tunnel", "DROP TUNNEL IF EXISTS my_tunnel"];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn drop_credentials_roundtrips() {
        let test_cases = [
            "DROP CREDENTIALS my_credentials",
            "DROP CREDENTIALS IF EXISTS my_credentials",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn alter_tunnel_roundtrips() {
        let test_cases = [
            "ALTER TUNNEL my_tunnel ROTATE KEYS",
            "ALTER TUNNEL IF EXISTS my_tunnel ROTATE KEYS",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn alter_database_roundtrips() {
        let test_cases = ["ALTER DATABASE my_db RENAME TO your_db"];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn copy_to_roundtrips() {
        let test_cases = [
            "COPY table TO 's3://bucket'",
            "COPY table TO 's3://bucket' OPTIONS (option1 = 'true', option2 = 'hello')",
            "COPY (SELECT 1) TO 's3://bucket'",
            "COPY (SELECT 1) TO 's3://bucket' OPTIONS (option1 = 'true', option2 = 'hello')",
            "COPY table TO 's3://bucket' OPTIONS (bool_opt = TRUE, num_opt = 1)",
            "COPY table TO 's3://bucket' FORMAT JSON",
            "COPY table TO 's3://bucket' CREDENTIALS aws_creds",
            "COPY table TO 's3://bucket' FORMAT JSON CREDENTIALS aws_creds",
            "COPY table TO s3 OPTIONS (creds = 'something')",
        ];

        for test_case in test_cases {
            let stmt = CustomParser::parse_sql(test_case)
                .unwrap()
                .pop_front()
                .unwrap();
            assert_eq!(test_case, stmt.to_string().as_str());
        }
    }

    #[test]
    fn options_parse() {
        let mut options = BTreeMap::new();
        options.insert(
            "o1".to_string(),
            OptionValue::QuotedLiteral("abc".to_string()),
        );
        options.insert("o2".to_string(), OptionValue::Boolean(true));
        options.insert(
            "o3".to_string(),
            OptionValue::UnquotedLiteral("def".to_string()),
        );
        options.insert("o4".to_string(), OptionValue::Number("123".to_string()));
        options.insert(
            "o5".to_string(),
            OptionValue::Secret("super_secret".to_string()),
        );

        let test_cases = [
            (
                "OPTIONS (
                    o1 = 'abc',
                    o2 = TRUE,
                    o3 = def,
                    o4 = 123,
                    o5 = SECRET super_secret
                )",
                options.clone(),
            ),
            (
                "(
                    o1 = 'abc',
                    o2 = TRUE,
                    o3 = def,
                    o4 = 123,
                    o5 = SECRET super_secret
                )",
                options.clone(),
            ),
            (
                "OPTIONS (
                    o1 'abc',
                    o2 TRUE,
                    o3 def,
                    o4 123,
                    o5 SECRET super_secret
                )",
                options.clone(),
            ),
            (
                "(
                    o1 'abc',
                    o2 TRUE,
                    o3 def,
                    o4 123,
                    o5 SECRET super_secret
                )",
                options.clone(),
            ),
            // Trailing comma
            (
                "OPTIONS (
                    o1 = 'abc',
                    o2 = TRUE,
                    o3 = def,
                    o4 = 123,
                    o5 = SECRET super_secret,
                )",
                options.clone(),
            ),
            (
                "(
                    o1 = 'abc',
                    o2 = TRUE,
                    o3 = def,
                    o4 = 123,
                    o5 = SECRET super_secret,
                )",
                options.clone(),
            ),
            (
                "OPTIONS (
                    o1 'abc',
                    o2 TRUE,
                    o3 def,
                    o4 123,
                    o5 SECRET super_secret,
                )",
                options.clone(),
            ),
            (
                "(
                    o1 'abc',
                    o2 TRUE,
                    o3 def,
                    o4 123,
                    o5 SECRET super_secret,
                )",
                options.clone(),
            ),
            // Empty
            ("", BTreeMap::new()),
            ("OPTIONS ( )", BTreeMap::new()),
            ("()", BTreeMap::new()),
        ];

        for (sql, map) in test_cases {
            let d = GenericDialect {};
            let t = Tokenizer::new(&d, sql).tokenize().unwrap();
            let mut p = CustomParser {
                parser: Parser::new(&d).with_tokens(t),
            };
            let opts = p.parse_options().unwrap();
            let expected_opts = StmtOptions::new(map);
            assert_eq!(opts, expected_opts);
        }
    }
}
