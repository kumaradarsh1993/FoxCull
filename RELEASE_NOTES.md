# FoxCull v1.2.0-nightly.4

Casting. You described three different misbehaviours — they turned out to be
three real bugs, and they explain each other.

## Why the TV stopped following you

**The receiver app on the TV is not permanent.** It closes itself whenever it
goes idle — a clip ends, a photo has been up a while, someone touches Home.
FoxCull asked the TV to launch it exactly once, when you first connected, and
never again. So the moment it closed, every later "show this one" had nowhere to
go and simply queued up forever. The connection was still alive, so the button
happily kept saying *Casting to Sony TV* while nothing could reach the screen
again. **That's the disappearing act — and once it happened, that session was
finished.**

**FoxCull also believed its own optimism.** It marked a file as "now on the TV"
the moment it *decided* to send it, not when it actually went out. So when a
send silently went nowhere, the follow logic thought it had done its job and
stopped trying — leaving the previous video playing on the TV while the app had
moved on. **That's the second thing you saw.**

**And two quick presses of → could land out of order**, because for RAW and HEIC
shots FoxCull has to build a preview first, which takes far longer than for a
photo already cached. The slower one could finish last and win. **That's why it
felt random.**

Now: the receiver is relaunched whenever it's needed and the media you asked for
is delivered as soon as it's back; a file counts as "on the TV" only once it has
genuinely been sent; and a superseded request is dropped instead of racing.
FoxCull also checks every couple of seconds that the session is really alive, so
the Cast button stops claiming a connection that has ended.

## Your XPS 13 question

Honest answer: **I don't know yet, and I'm not going to guess.** What I can tell
you is what it depends on. Chrome has no software HEVC decoder at all, so this
works only where the graphics chip does it — and Intel's built-in graphics gained
10-bit HEVC decode in the 7th generation (2017). Your Osmo footage is 10-bit. So
it comes down to that laptop's vintage, which isn't written down anywhere.

So I made the app answer it. Every clip you open now writes one line to
`foxcull.log` saying whether the decoder took it and why not if it didn't. Open a
few clips on the XPS, send me the log, and you'll have a real answer instead of
my opinion.

If it turns out not to be supported there, nothing breaks: Glimpse greys out with
an explanation and scrubbing falls back to the older behaviour.

## Still worth testing

Glimpse's pacing at the default 40×, and phone (H.264) video, which takes a
slightly different path inside and still hasn't met a real file.
