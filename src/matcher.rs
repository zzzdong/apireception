use headers::{Cookie, HeaderMapExt};
use hyper::{header::HOST, Body, Method};
use nom::{
    branch::alt,
    bytes::{complete::tag, complete::take_while},
    combinator::{eof, map_res},
    sequence::{delimited, separated_pair},
    IResult,
};
use regex::Regex;
use std::{collections::HashMap, convert::TryFrom, ops::Deref};

use crate::error::MatcherParseError;

const ESCAPE_CHARS: &str = r#"\'"()"#;

#[derive(Debug, Clone)]
pub struct ComparableRegex(Regex);

impl ComparableRegex {
    pub fn new(re: &str) -> Result<Self, regex::Error> {
        Ok(ComparableRegex(Regex::new(re)?))
    }
}

impl PartialEq for ComparableRegex {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

/// neat trick for making all functions of the internal type available
impl Deref for ComparableRegex {
    type Target = Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RouteMatcher {
    Method(Method),
    Host(String),
    HostRegexp(ComparableRegex),
    Path(String),
    PathRegexp(ComparableRegex),
    Query(String, String),
    Cookie(String, String),
    And(Box<RouteMatcher>, Box<RouteMatcher>),
    Or(Box<RouteMatcher>, Box<RouteMatcher>),
    Empty,
}

impl RouteMatcher {
    pub fn parse(i: &str) -> Result<RouteMatcher, MatcherParseError> {
        if i.is_empty() || i.trim().is_empty() {
            return Ok(RouteMatcher::Empty);
        }

        let (_i, matcher) = top_level(i).map_err(|e| MatcherParseError::new(e.to_string()))?;
        Ok(matcher)
    }

    pub fn matchs(&self, req: &hyper::Request<Body>) -> bool {
        match self {
            RouteMatcher::Method(method) => req.method() == method,
            RouteMatcher::Host(host) => req.headers().get(HOST).map(|h| h == host).unwrap_or(false),
            RouteMatcher::HostRegexp(host_regex) => req
                .headers()
                .get(HOST)
                .and_then(|h| Some(host_regex.is_match(h.to_str().ok()?)))
                .unwrap_or(false),
            RouteMatcher::Path(path) => req.uri().path() == path,
            RouteMatcher::PathRegexp(path_regex) => path_regex.is_match(req.uri().path()),
            RouteMatcher::Query(key, value) => {
                let query_params: HashMap<String, String> = req
                    .uri()
                    .query()
                    .map(|v| {
                        url::form_urlencoded::parse(v.as_bytes())
                            .into_owned()
                            .collect()
                    })
                    .unwrap_or_else(HashMap::new);

                query_params
                    .get(key)
                    .map(|sent_value| sent_value == value)
                    .unwrap_or(false)
            }
            RouteMatcher::Cookie(key, value) => req
                .headers()
                .typed_get::<Cookie>()
                .map(|cookie| cookie.get(key) == Some(value))
                .unwrap_or(false),
            RouteMatcher::And(lhs, rhs) => lhs.matchs(req) && rhs.matchs(req),
            RouteMatcher::Or(lhs, rhs) => lhs.matchs(req) || rhs.matchs(req),
            RouteMatcher::Empty => true,
        }
    }
}

fn in_quotes(input: &str) -> IResult<&str, String> {
    let mut ret = String::new();
    let mut iter = input.chars().peekable();
    let mut offset = 0;

    loop {
        match iter.next() {
            Some('\'') => {
                return Ok((&input[offset..], ret));
            }
            Some('\\') => {
                let ch = iter
                    .peek()
                    .ok_or(nom::Err::Incomplete(nom::Needed::Unknown))?;

                if ESCAPE_CHARS.contains(*ch) {
                    ret.push(iter.next().unwrap());
                    offset += 1;
                }
            }
            Some(ch) => {
                ret.push(ch);
            }
            None => {
                return Err(nom::Err::Incomplete(nom::Needed::Unknown));
            }
        }
        offset += 1;
    }
}

fn key_value(i: &str) -> IResult<&str, (String, String)> {
    separated_pair(parse_str, tag(","), parse_str)(i)
}

fn sp(i: &str) -> IResult<&str, &str> {
    let chars = " \t\r\n";

    take_while(move |c| chars.contains(c))(i)
}

fn parse_single_quoted(input: &str) -> IResult<&str, String> {
    delimited(tag("\'"), in_quotes, tag("\'"))(input)
}

fn parse_str(i: &str) -> IResult<&str, String> {
    delimited(sp, parse_single_quoted, sp)(i)
}

fn host(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, s) = delimited(tag("Host("), parse_str, tag(")"))(i)?;

    Ok((i, RouteMatcher::Host(s)))
}

fn host_regexp(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, regexp) = map_res(
        delimited(tag("HostRegexp("), parse_str, tag(")")),
        |s: String| ComparableRegex::new(&s),
    )(i)?;

    Ok((i, RouteMatcher::HostRegexp(regexp)))
}

fn method(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, m) = map_res(
        delimited(tag("Method("), parse_str, tag(")")),
        |s: String| Method::try_from(s.as_str()),
    )(i)?;

    Ok((i, RouteMatcher::Method(m)))
}

fn path(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, s) = delimited(tag("Path("), parse_str, tag(")"))(i)?;

    Ok((i, RouteMatcher::Path(s)))
}

fn path_regexp(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, regexp) = map_res(
        delimited(tag("PathRegexp("), parse_str, tag(")")),
        |s: String| ComparableRegex::new(&s),
    )(i)?;

    Ok((i, RouteMatcher::PathRegexp(regexp)))
}

fn query(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, (k, v)) = delimited(tag("Query("), key_value, tag(")"))(i)?;

    Ok((i, RouteMatcher::Query(k, v)))
}

fn cookie(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, (k, v)) = delimited(tag("Cookie("), key_value, tag(")"))(i)?;

    Ok((i, RouteMatcher::Cookie(k, v)))
}

fn and(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, (lhs, rhs)) = separated_pair(value, tag("&&"), value)(i)?;

    Ok((i, RouteMatcher::And(Box::new(lhs), Box::new(rhs))))
}

fn or(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, (lhs, rhs)) = separated_pair(value, tag("||"), value)(i)?;

    Ok((i, RouteMatcher::Or(Box::new(lhs), Box::new(rhs))))
}

fn chained(i: &str) -> IResult<&str, RouteMatcher> {
    alt((and, or))(i)
}

fn value(i: &str) -> IResult<&str, RouteMatcher> {
    let nested = delimited(tag("("), alt((chained, value)), tag(")"));

    delimited(
        sp,
        alt((
            host,
            host_regexp,
            path,
            path_regexp,
            method,
            query,
            cookie,
            nested,
        )),
        sp,
    )(i)
}

fn top_level(i: &str) -> IResult<&str, RouteMatcher> {
    let (i, m) = alt((chained, value))(i)?;
    let (i, _) = eof(i)?;

    Ok((i, m))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_matcher() {
        let input = "Cookie('env','dev')";
        let matcher = RouteMatcher::parse(input).unwrap();

        let req = hyper::Request::builder()
            .header("Cookie", "env=dev")
            .body(Body::empty())
            .unwrap();

        assert_eq!(matcher.matchs(&req), true);
    }

    #[test]
    fn parse_host() {
        let input = "Host('www.google.com')";

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Host("www.google.com".to_string()))
        );
    }

    #[test]
    fn parse_escaped_host() {
        let input = "Host('www.\\'google.com')";

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Host("www.'google.com".to_string()))
        );

        let input = r#"Host('www.\'go\"ogle.\\com')"#;

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Host(r#"www.'go"ogle.\com"#.to_string()))
        );
    }

    #[test]
    fn parse_empty_host() {
        let input = "Host('')";

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Host("".to_string()))
        );
    }

    #[test]
    fn parse_host_regexp() {
        let input = "HostRegexp('[0-9]+')";

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::HostRegexp(
                ComparableRegex::new("[0-9]+").unwrap()
            ))
        );
    }

    #[test]
    fn parse_path_regexp() {
        let input = r#"PathRegexp('/hello/\(.*\)')"#;

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::PathRegexp(
                ComparableRegex::new("/hello/(.*)").unwrap()
            ))
        );
    }

    #[test]
    fn parse_query() {
        let input = "Query( 'key' , 'value' )";

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Query("key".into(), "value".into()))
        );
    }

    #[test]
    fn parse_cookie() {
        let input = "Cookie( 'key' , 'value' )";

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Cookie("key".into(), "value".into()))
        );
    }

    #[test]
    fn parse_and() {
        let input = "Host('www.google.com') && Path('/api/user')";

        let lhs = Box::new(RouteMatcher::Host("www.google.com".to_string()));
        let rhs = Box::new(RouteMatcher::Path("/api/user".to_string()));

        assert_eq!(RouteMatcher::parse(input), Ok(RouteMatcher::And(lhs, rhs)));
    }

    #[test]
    fn parse_or() {
        let input = "Host('www.google.com')||Path('/api/admin/')";

        let lhs = Box::new(RouteMatcher::Host("www.google.com".to_string()));
        let rhs = Box::new(RouteMatcher::Path("/api/admin/".to_string()));

        assert_eq!(RouteMatcher::parse(input), Ok(RouteMatcher::Or(lhs, rhs)));
    }

    #[test]
    fn parse_chained() {
        let input = "(Path('/api/admin/')||Path('/api/manage/'))";

        let admin_path = Box::new(RouteMatcher::Path("/api/admin/".to_string()));
        let manage_path = Box::new(RouteMatcher::Path("/api/manage/".to_string()));

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::Or(admin_path, manage_path))
        );

        let input = "Host( 'www.google.com')&&( Path('/api/admin/') || Path('/api/manage/') )";

        let host = Box::new(RouteMatcher::Host("www.google.com".to_string()));
        let admin_path = Box::new(RouteMatcher::Path("/api/admin/".to_string()));
        let manage_path = Box::new(RouteMatcher::Path("/api/manage/".to_string()));
        let path = Box::new(RouteMatcher::Or(admin_path, manage_path));

        assert_eq!(
            RouteMatcher::parse(input),
            Ok(RouteMatcher::And(host, path))
        );
    }
}
