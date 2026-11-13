// SPDX-License-Identifier: MIT OR Apache-2.0
// Negative test NEG-1: Go reads the clock via the vDSO, bypassing LD_PRELOAD.
package main

import (
	"fmt"
	"time"
)

func main() { fmt.Println(time.Now().Unix()) }
