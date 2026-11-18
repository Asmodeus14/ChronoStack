<?php
// SPDX-License-Identifier: MIT OR Apache-2.0
// No-hang checks for PHP: every wait below has a 1 s timeout and must return
// after about 1 s of real time while virtual time is far ahead. PHP has no
// threads, so each wait runs in its own `php` process and this parent stops
// it after 6 s (HANG). Prints one JSON line per primitive.

if (($argv[1] ?? '') === 'now') {
    echo json_encode(['now' => microtime(true)]), "\n";
    exit(0);
}

$tests = [
    'sleep' => 'sleep(1);',
    'usleep' => 'usleep(1000000);',
    'time_nanosleep' => 'time_nanosleep(1, 0);',
    'stream_select' => '$p = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);'
        . ' $r = [$p[0]]; $w = null; $e = null; stream_select($r, $w, $e, 1);',
    'fread_stream_timeout' => '$p = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);'
        . ' stream_set_timeout($p[0], 1); fread($p[0], 1);',
];

foreach ($tests as $name => $code) {
    $t0 = hrtime(true);
    $proc = proc_open([PHP_BINARY, '-r', $code], [], $pipes);
    $finished = false;
    while ((hrtime(true) - $t0) < 6e9) {
        if (!proc_get_status($proc)['running']) {
            $finished = true;
            break;
        }
        usleep(10000);
    }
    $ms = (hrtime(true) - $t0) / 1e6;
    if (!$finished) {
        proc_terminate($proc, 9);
    }
    proc_close($proc);
    $status = !$finished ? 'HANG' : ($ms < 900 ? 'EARLY' : ($ms > 2500 ? 'SLOW' : 'OK'));
    echo json_encode(['test' => $name, 'elapsed_ms' => (int) round($ms), 'status' => $status]), "\n";
    flush();
}
