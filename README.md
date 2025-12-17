## Overview

[![Build status](https://img.shields.io/github/actions/workflow/status/travisbrown/cancel-culture/ci.yaml?branch=main)](https://github.com/travisbrown/cancel-culture/actions)
[![Coverage status](https://img.shields.io/codecov/c/github/travisbrown/cancel-culture/main.svg)](https://codecov.io/github/travisbrown/cancel-culture)

This repository contains some low-tech tools designed to help you make Twitter a nicer place for
yourself. Some of these tools might also be useful in other ways, like for
example if some litigious person with a long history of making common cause with white
supremacists and misogynists
[threatens to sue you for defamation](https://meta.plasm.us/posts/2020/07/25/response-to-john-de-goes/).

See [this related project](https://github.com/travisbrown/deleted-tweets) for an example of the
kind of use case cancel-culture is designed to support (an archive of around 35 million deleted
tweets associated with Gamergate, [LambdaConf](https://geekfeminism.wikia.org/wiki/Lambdaconf_incident), Stop the Steal, etc.),
or [this project](https://github.com/travisbrown/evasion) focused on tracking ban evasion by far-right accounts,
or [this recent project](https://github.com/salcoast/deleted-tweets-archive) by
[Salish Coast Anti-Fascist Action](https://twitter.com/SalishcoastA).

## Testimonials

[A Twitter user](https://web.archive.org/web/20210930150828/https://twitter.com/gringovice/status/1443551046823989256):

> Still, he somehow has access to everything you’ve ever posted & deleted, and can seemingly immediately find your new alt/resurrect/punished accounts.

## Terms of service compliance

This software is designed to promote use that is compliant with the Twitter API
[Developer Agreement](https://developer.twitter.com/en/developer-terms/agreement-and-policy)
and the [Internet Archive](https://archive.org/)'s [Terms of Use](https://archive.org/about/terms.php).

Text and metadata for Twitter statuses are retrieved from the [Wayback Machine][wayback-machine], not the Twitter API,
which is primarily used here to list follower relationships and to allow users to import and export block lists.

In theory it's possible that there are ways you could violate the Developer Agreement with the help of this software (for example
by using "information obtained from the Twitter API to target people with advertising outside of the Twitter platform").
Don't do that.

## Examples

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
provides a way to search the [Wayback Machine][wayback-machine] for deleted tweets by a
specified user:

```
$ target/release/twcc deleted-tweets --limit 100 jdegoes
https://web.archive.org/web/20190922222236/https://twitter.com/jdegoes/status/1170420726400212997
https://web.archive.org/web/20190923221242/https://twitter.com/jdegoes/status/1170711737361940481
https://web.archive.org/web/20200526150339/https://twitter.com/jdegoes/status/1265251872048320513
```

In this case we've limited the search to the 100 tweets most recently archived by the Wayback
Machine.

You can also use this command to generate a Markdown-formatted report instead of a simple list of
links:

```
$ target/release/twcc deleted-tweets --report ChiefScientist
```

Which currently generates [this document](https://gist.github.com/travisbrown/9ca0dafe086e4904480b91d5019de96d).

It can also print a list of everyone you currently block, follow, or are followed by, it can get the
URL of a deleted tweet from the URL of a reply, and it can partition a list of tweet IDs by their
deleted status.

```
$ target/debug/twcc --help
Usage: twcc [OPTIONS] <COMMAND>

Commands:
  blocked-follows   For a given user, list everyone they follow who you block
  follower-report   For a given user, print a report about their followers
  lookup-reply      Get the URL of a tweet given the URL or status ID of a reply
  check-existence   Check whether a list of status IDs (from stdin) still exist
  deleted-tweets    List Wayback Machine URLs for all deleted tweets by a user
  list-followers    Print a list of all users who follow you (or someone else)
  list-friends      Print a list of all users you (or someone else) follows
  list-blocks       Print a list of all users you've blocked
  list-tweets       Print a list of (up to approximately 3200) tweet IDs for a user
  list-tweets-json
  lookup-tweets     Read tweet IDs from stdin and print info
  import-blocks     Block a list of user IDs (from stdin)
  list-unmutuals    List everyone you follow or who follows you who is not a mutual
  help              Print this message or the help of the given subcommand(s)

Options:
  -k, --key-file <KEY_FILE>
          TOML file containing Twitter API keys

          [default: keys.toml]

  -v, --verbose...
          Level of verbosity

      --wayback-pacing <WAYBACK_PACING>
          Wayback pacing profile (controls how fast we query/download from the Wayback Machine)

          Possible values:
          - conservative: Conservative pacing intended to minimize the risk of Wayback CDX throttling
          - default:      Default pacing (balanced)
          - adaptive:     Adaptive pacing with hysteresis (slow recovery, fast backoff)

          [default: default]
```

In `deleted-tweets` mode, you can also run without Twitter API checks:

```
$ target/release/twcc deleted-tweets --no-api --report <screen-name>
```

By default, report generation overlaps a small number of Wayback downloads to reduce the impact of
network latency. You can adjust how many downloads are in flight with `--content-concurrency`:

```
$ target/release/twcc deleted-tweets --report --content-concurrency 4 <screen-name>
```

## Tweet screenshots

The `twshoot` command-line tool can take a screenshot of a tweet, given either a URL or status ID:

```
$ cargo build --release
    Finished release [optimized] target(s) in 0.29s

$ target/release/twshoot https://twitter.com/travisbrown/status/1291256191641952256
```

And then you have a `1291256191641952256.png` file in the current directory that looks like this:

<p align="center">
<img
  alt="Liking Scala is not a personality but it does mean you're racist / Do I think this is 100% accurate or fair: no … Do I think the Scala community is capable of coming to terms with the behavior that got it this reputation: also no"
  src="/examples/1291256191641952256.png?raw=true"
  width="75%"
  />
</p>

The application also generates a `-full.png` image showing the entire browser screen. The image
sizes, output directory, etc. are configurable (see `twshoot --help` for details).

This tool doesn't require a Twitter API account, but you do have to have
[ChromeDriver](https://chromedriver.chromium.org/) running (it also works with GeckoDriver,
but the results don't look as nice).

## Setup

You'll need to [install Rust and Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html).

Once you've got those, you can run `cargo build --release` and the binaries will be available in the
`target/release` directory.

For the main `twcc` application, some commands use the Twitter API and require
[Twitter API access](https://developer.twitter.com/en/apply-for-access) plus a keys file (by
default
`keys.toml`):

```toml
[twitter]
consumerKey = "****"
consumerSecret = "****"
accessToken = "****"
accessTokenSecret = "****"
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

I might add some of them eventually.

Nothing here is very polished or robust. Some commands may still run into rate limits or transient
failures and require waiting/retrying. I might try to smooth out some of these rough edges at some
point, but it's unlikely.

## License

This project is licensed under the Mozilla Public License, version 2.0. See the LICENSE file for details.

[wayback-machine]: https://web.archive.org/
