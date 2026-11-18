# SPDX-License-Identifier: MIT OR Apache-2.0
# Run once: ruby signup.rb ada@example.com
require "./jobs"

TrialExpiredJob.perform_in(14 * 24 * 60 * 60, ARGV.fetch(0, "ada@example.com"))
puts "scheduled the trial-expired email for #{ARGV.fetch(0, 'ada@example.com')} in 14 days"
