/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/* Negative test NEG-2: a statically linked binary cannot be preloaded. */
#include <stdio.h>
#include <time.h>
int main(void) { printf("%lld\n", (long long)time(NULL)); return 0; }
