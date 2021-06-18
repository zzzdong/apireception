use nom::{
    branch::alt,
    bytes::{complete::tag, complete::take_while},
    combinator::map_res,
    sequence::{delimited, separated_pair},
    IResult,
};
use regex::Regex;

#[derive(Debug)]
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

#[derive(Debug, PartialEq)]
pub enum RuleMatcher {
    Host(String),
    HostRegexp(ComparableRegex),
    Method(String),
    Path(String),
    PathRegexp(ComparableRegex),
    Query(String, String),
    Cookie(String, String),
    And(Box<RuleMatcher>, Box<RuleMatcher>),
    Or(Box<RuleMatcher>, Box<RuleMatcher>),
}

impl RuleMatcher {
    pub fn parse(i: &str) -> Result<RuleMatcher, String> {
        let (i, matcher) = top_level(i).map_err(|e| e.to_string())?;

        if !i.is_empty() {
            return Err("can't parse all content".to_string());
        }

        Ok(matcher)
    }
}

fn in_quotes(buf: &str) -> IResult<&str, String> {
    let mut ret = String::new();
    let mut should_escape = false;
    for (i, ch) in buf.chars().enumerate() {
        if ch == '\'' && !should_escape {
            return Ok((&buf[i..], ret));
        }
        if ch == '\\' && !should_escape {
            should_escape = true;
        } else if r#"\'""#.contains(ch) && should_escape {
            ret.push(ch);
            should_escape = false;
        } else {
            ret.push(ch);
            should_escape = false;
        }
    }
    Err(nom::Err::Incomplete(nom::Needed::Unknown))
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

fn host(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, s) = delimited(tag("Host("), parse_str, tag(")"))(i)?;

    Ok((i, RuleMatcher::Host(s.to_string())))
}

fn host_regexp(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, regexp) = map_res(
        delimited(tag("HostRegexp("), parse_str, tag(")")),
        |s: String| ComparableRegex::new(&s),
    )(i)?;

    Ok((i, RuleMatcher::HostRegexp(regexp)))
}

fn method(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, s) = delimited(tag("Method("), parse_str, tag(")"))(i)?;

    Ok((i, RuleMatcher::Method(s.to_string())))
}

fn path(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, s) = delimited(tag("Path("), parse_str, tag(")"))(i)?;

    Ok((i, RuleMatcher::Path(s.to_string())))
}

fn path_regexp(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, regexp) = map_res(
        delimited(tag("PathRegexp("), parse_str, tag(")")),
        |s: String| ComparableRegex::new(&s),
    )(i)?;

    Ok((i, RuleMatcher::PathRegexp(regexp)))
}

fn query(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, (k, v)) = delimited(tag("Query("), key_value, tag(")"))(i)?;

    Ok((i, RuleMatcher::Query(k, v)))
}

fn and(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, (lhs, rhs)) = separated_pair(value, tag("&&"), value)(i)?;

    Ok((i, RuleMatcher::And(Box::new(lhs), Box::new(rhs))))
}

fn or(i: &str) -> IResult<&str, RuleMatcher> {
    let (i, (lhs, rhs)) = separated_pair(value, tag("||"), value)(i)?;

    Ok((i, RuleMatcher::Or(Box::new(lhs), Box::new(rhs))))
}

fn chained(i: &str) -> IResult<&str, RuleMatcher> {
    alt((and, or))(i)
}

fn value(i: &str) -> IResult<&str, RuleMatcher> {
    let nested = delimited(tag("("), alt((chained, value)), tag(")"));

    delimited(
        sp,
        alt((host, host_regexp, path, path_regexp, method, query, nested)),
        sp,
    )(i)
}

fn top_level(i: &str) -> IResult<&str, RuleMatcher> {
    alt((chained, value))(i)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_host() {
        let input = "Host('www.google.com')";

        assert_eq!(
            RuleMatcher::parse(input),
            Ok(RuleMatcher::Host("www.google.com".to_string()))
        );
    }

    #[test]
    fn parse_escaped_host() {
        let input = "Host('www.\\'google.com')";

        assert_eq!(
            RuleMatcher::parse(input),
            Ok(RuleMatcher::Host("www.'google.com".to_string()))
        );
    }

    #[test]
    fn parse_empty_host() {
        let input = "Host('')";

        assert_eq!(
            RuleMatcher::parse(input),
            Ok(RuleMatcher::Host("".to_string()))
        );
    }

    #[test]
    fn parse_host_regexp() {
        let input = "HostRegexp('[0-9]+')";

        assert_eq!(
            RuleMatcher::parse(input),
            Ok(RuleMatcher::HostRegexp(
                ComparableRegex::new("[0-9]+").unwrap()
            ))
        );
    }

    #[test]
    fn parse_query() {
        let input = "Query( 'key' , 'value' )";

        assert_eq!(
            RuleMatcher::parse(input),
            Ok(RuleMatcher::Query("key".into(), "value".into()))
        );
    }

    #[test]
    fn parse_and() {
        let input = "Host('www.google.com') && Path('/api/user')";

        let lhs = Box::new(RuleMatcher::Host("www.google.com".to_string()));
        let rhs = Box::new(RuleMatcher::Path("/api/user".to_string()));

        assert_eq!(RuleMatcher::parse(input), Ok(RuleMatcher::And(lhs, rhs)));
    }

    #[test]
    fn parse_or() {
        let input = "Host('www.google.com')||Path('/api/admin/')";

        let lhs = Box::new(RuleMatcher::Host("www.google.com".to_string()));
        let rhs = Box::new(RuleMatcher::Path("/api/admin/".to_string()));

        assert_eq!(RuleMatcher::parse(input), Ok(RuleMatcher::Or(lhs, rhs)));
    }

    #[test]
    fn parse_chained() {
        let input = "(Path('/api/admin/')||Path('/api/manage/'))";

        let admin_path = Box::new(RuleMatcher::Path("/api/admin/".to_string()));
        let manage_path = Box::new(RuleMatcher::Path("/api/manage/".to_string()));

        assert_eq!(
            RuleMatcher::parse(input),
            Ok(RuleMatcher::Or(admin_path, manage_path))
        );

        let input = "Host( 'www.google.com')&&( Path('/api/admin/') || Path('/api/manage/') )";

        let host = Box::new(RuleMatcher::Host("www.google.com".to_string()));
        let admin_path = Box::new(RuleMatcher::Path("/api/admin/".to_string()));
        let manage_path = Box::new(RuleMatcher::Path("/api/manage/".to_string()));
        let path = Box::new(RuleMatcher::Or(admin_path, manage_path));

        assert_eq!(RuleMatcher::parse(input), Ok(RuleMatcher::And(host, path)));
    }
}
