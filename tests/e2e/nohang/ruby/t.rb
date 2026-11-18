# SPDX-License-Identifier: MIT OR Apache-2.0
# No-hang checks for Ruby (MRI): every wait below has a 1 s timeout and must
# return after about 1 s of real time while virtual time is far ahead.
# Prints one JSON line per primitive: {"test", "elapsed_ms", "status"}.
require "json"
require "timeout"

if ARGV[0] == "now"
  puts JSON.generate({ now: Time.now.to_f })
  exit 0
end

def mono
  Process.clock_gettime(Process::CLOCK_MONOTONIC)
end

held = Mutex.new
Thread.new { held.lock; sleep }
sleep 0.1

TESTS = {
  "Kernel#sleep" => -> { sleep 1 },
  "ConditionVariable#wait" => -> { m = Mutex.new; cv = ConditionVariable.new; m.synchronize { cv.wait(m, 1) } },
  "Queue#pop_timeout" => -> { Queue.new.pop(timeout: 1) },
  "Thread#join_timeout" => -> { Thread.new { sleep 10 }.join(1) },
  "IO.select" => -> { r, _w = IO.pipe; IO.select([r], nil, nil, 1) },
  "Timeout.timeout" => -> { begin; Timeout.timeout(1) { held.lock }; rescue Timeout::Error; end },
}

TESTS.each do |name, fn|
  done = Queue.new
  t0 = mono
  Thread.new { fn.call; done << true }
  finished = done.pop(timeout: 6)
  ms = (mono - t0) * 1000
  status = if finished.nil? then "HANG" elsif ms < 900 then "EARLY" elsif ms > 2500 then "SLOW" else "OK" end
  puts JSON.generate({ test: name, elapsed_ms: ms.round, status: status })
  $stdout.flush
end
exit!(0)
