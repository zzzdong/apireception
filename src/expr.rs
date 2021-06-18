use nom::{
    branch::alt,
    bytes::{
        complete::tag,
        complete::{take, take_while},
    },
    character::complete::char,
    character::{
        complete::{alpha0, alpha1, alphanumeric1, one_of},
        is_alphabetic,
    },
    combinator::{map_res, opt, recognize, value},
    multi::{many0, many1},
    sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
    IResult,
};
use regex::Regex;

struct Engine {}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Null,
}

#[derive(Debug, Clone)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Re,
    Rn,
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

fn sp(i: &str) -> IResult<&str, &str> {
    let chars = " \t\r\n";

    take_while(move |c| chars.contains(c))(i)
}

fn value_null(i: &str) -> IResult<&str, Value> {
    let (i, null) = tag("null")(i)?;

    Ok((i, Value::Null))
}

fn parse_single_quoted(input: &str) -> IResult<&str, String> {
    delimited(tag("\'"), in_quotes, tag("\'"))(input)
}

fn parse_str(i: &str) -> IResult<&str, String> {
    delimited(sp, parse_single_quoted, sp)(i)
}

fn value_string(i: &str) -> IResult<&str, Value> {
    let (i, s) = parse_str(i)?;

    Ok((i, Value::String(s.to_string())))
}

fn float(input: &str) -> IResult<&str, &str> {
    alt((
        // Case one: .42
        recognize(tuple((
            char('.'),
            decimal,
            opt(tuple((one_of("eE"), opt(one_of("+-")), decimal))),
        ))), // Case two: 42e42 and 42.42e42
        recognize(tuple((
            decimal,
            opt(preceded(char('.'), decimal)),
            one_of("eE"),
            opt(one_of("+-")),
            decimal,
        ))), // Case three: 42. and 42.42
        recognize(tuple((decimal, char('.'), opt(decimal)))),
    ))(input)
}

fn value_float(i: &str) -> IResult<&str, Value> {
    let (i, f) = map_res(float, |s: &str| s.parse::<f64>())(i)?;

    Ok((i, Value::Float(f)))
}

fn decimal(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        many0(one_of("+-")),
        many1(terminated(one_of("0123456789"), many0(char('_')))),
    ))(input)
}

fn value_int(i: &str) -> IResult<&str, Value> {
    let (i, d) = map_res(decimal, |s: &str| s.parse::<i64>())(i)?;

    Ok((i, Value::Int(d)))
}

fn parse_value(i: &str) -> IResult<&str, Value> {
    alt((value_null, value_string, value_float, value_int))(i)
}

fn ident(i: &str) -> IResult<&str, &str> {
    recognize(pair(
        alt((alpha1, tag("_"))),
        many0(alt((alphanumeric1, tag("_"), tag("-")))),
    ))(i)
}

fn op(i: &str) -> IResult<&str, Op> {
    alt((
        value(Op::Eq, tag("=")),
        value(Op::Ne, tag("!=")),
        value(Op::Lt, tag("<")),
        value(Op::Le, tag("<=")),
        value(Op::Gt, tag(">")),
        value(Op::Ge, tag(">=")),
        value(Op::Re, tag("!~")),
        value(Op::Rn, tag("=~")),
    ))(i)
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse_value(i: &str) -> Result<Value, String> {
        let (_i, v) = super::parse_value(i).map_err(|e| e.to_string())?;

        Ok(v)
    }

    #[test]
    fn test_parse_value() {
        assert_eq!(parse_value("123"), Ok(Value::Int(123)));
        assert_eq!(parse_value("-123"), Ok(Value::Int(-123)));
        assert_eq!(parse_value("+123"), Ok(Value::Int(123)));
        assert_eq!(parse_value("123.0"), Ok(Value::Float(123.0)));
        assert_eq!(parse_value("-123.0"), Ok(Value::Float(-123.0)));

        assert_eq!(parse_value("'header'"), Ok(Value::String("header".to_string())));
        assert_eq!(parse_value("null"), Ok(Value::Null));
    }
}
