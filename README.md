## Overview

This repository contains some low-tech tools designed to help you make Twitter a nicer place for
yourself. Some of these tools might also be useful in other ways, like for
example if some litigious person with a long history of making common cause with white
supremacists and misogynists
[threatens to sue you for defamation](https://meta.plasm.us/posts/2020/07/25/response-to-john-de-goes/).

One of the things this project provides is a command-line tool that takes a Twitter screen
name and outputs a list of all of the accounts you've blocked that that account follows (sorted
here by follower count):

```
$ cargo build --release
    Finished release [optimized] target(s) in 0.06s

$ target/release/twcc blocked-follows sfscala
@jdegoes             11807
@rolandkuhn           9238
@propensive           8323
@etorreborre          7045
@ChiefScientist       5450
@dibblego             3587
@nuttycom             3307
@kubukoz              2808
@scalaworldconf       2495
...
```

I get a lot of hate-follows, and this tool makes it much easier for me to decide which new followers
I need to block. It's like a version of Twitter's "Followed by… and 123 others you follow" that's
actually useful.

I sometimes work in a [certain programming language community](https://www.scala-lang.org) where
prominent community members have a tendency to say abusive
or exclusionary things and then delete and deny everything when they're confronted, so the CLI also
provides a way to search the [Wayback Machine](https://web.archive.org/) for deleted tweets by a
specified user:

```
$ target/release/twcc deleted-tweets --limit 100 jdegoes
https://web.archive.org/web/20190922222236/https://twitter.com/jdegoes/status/1170420726400212997
https://web.archive.org/web/20190923221242/https://twitter.com/jdegoes/status/1170711737361940481
https://web.archive.org/web/20200526150339/https://twitter.com/jdegoes/status/1265251872048320513
```

(In this case we've limited the search to the 100 tweets most recently archived by the Wayback
Machine.)

It can also print a list of everyone you currently block, follow, or are followed by, it can get the
URL of a deleted tweet from the URL of a reply, and it can partition a list of tweet IDs by their
deleted status.

```
twcc 0.1.0
Travis Brown <travisrobertbrown@gmail.com>

USAGE:
    twcc [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -k, --key-file <key-file>    TOML file containing Twitter API keys [default: keys.toml]

SUBCOMMANDS:
    blocked-follows    For a given user, list everyone they follow who you block
    check-existence    Checks whether a list of status IDs (from stdin) still exist
    deleted-tweets     Lists Wayback Machine URLs for all deleted tweets by a user
    help               Prints this message or the help of the given subcommand(s)
    list-blocks        Print a list of all users you've blocked
    list-followers     Print a list of all users who follow you (or someone else)
    list-friends       Print a list of all users you (or someone else) follows
    lookup-reply       Get the URL of a tweet given the URL or status ID of a reply
```

## Setup

You'll need to [install Rust and Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html).

Once you've got those, you can run `cargo build --release` and the binaries will be available in the
`target/release` directory.

For the main `twcc` application, you'll need
[Twitter API access](https://developer.twitter.com/en/apply-for-access)
for your Twitter account, and you'll need to provide the necessary keys in a file (by default
`keys.toml`):

```toml
[twitter]
consumerKey="****"
consumerSecret="****"
accessToken="****"
accessTokenSecret="****"
```

Some of the other tools require a [WebDriver](https://www.w3.org/TR/webdriver/) server instead of
API access. These should work with either [ChromeDriver](https://chromedriver.chromium.org/) or
[GeckoDriver](https://github.com/mozilla/geckodriver).

## Other

The project also contains some other miscellaneous stuff, including a way to export your Twitter
block list even if you don't have a Twitter API account, and a way to search the Wayback Machine
even if the CDX server isn't working.

It doesn't currently include a few related tools I use regularly, including a way to block everyone
who retweeted or favorited a given tweet, a bunch of stuff related to Wayback Machine ingestion
and downloading, and some scripts that bundle some of the follower functionality into daily reports.

Most of these things are excluded for one of the following reasons:

* They're even more fragile than the stuff that's here now.
* I haven't ported them from Scala to Rust yet.
* They probably technically violate some ToS somewhere.

I might add some of them eventually.

Nothing here is very polished or robust. These applications don't keep track of rate limits, for
example, so if you run out of requests for an endpoint, they'll just crash, and
you'll have to wait. I might try to smooth out some of these rough edges at some
point, but it's unlikely.
