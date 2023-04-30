# video-encoding-wrapper

A highly opinionated video encoding wrapper.

This tool is not intended for users who are limited on temporary disk space or
who are looking for a fast solution or who have a slow CPU. The vast majority of
users will be better served by Av1an.

## Overview

This tool, given a source video file on the command line and an output directory
(and perhaps some other configuration flags), executes the following steps:

1. Determines the frame count and optimal crop settings for the video.
2. Finds scene changes in the video.
3. Extracts and crops each scene to a separate FFV1-encoded file.
4. Encodes the scenes using the configured settings.
5. Merges the encoded scenes into a final combined output file.
6. Collects and prints various metrics of the final file.

There are a few caveats: First, it processes the single best video stream from
the original file and completely ignores any other streams. The user is expected
to manually add any additional streams they want in a postprocessing step.
Second, because the scenes are encoded to separate files, this does require
extra disk space. This is completely intentional to allow for repeated runs with
different encoding settings, and to make testing settings via manual use of the
relevant tools easier. This is the singlest biggest reason that Av1an will
probably better serve the majority of users.

Along with the above, all calculated data is cached and can be reused when
appropriate. Aborted encodes will automatically reuse previously existing
encoded video when it makes sense to do so. Notwithstanding a bug, and assuming
the user does not delete or modify the output directory, nothing will be
calculated twice, even in repeated runs.

More detailed instructions may appear here at a later date when the tool is more
mature, but then again, they may not.
