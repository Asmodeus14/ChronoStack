-- Prints how much Sidekiq work is pending: jobs in every queue, scheduled and
-- retry jobs that are due (by Redis's own, virtual, clock), and jobs running.
local t = redis.call('TIME')
local now = tonumber(t[1]) + tonumber(t[2]) / 1e6
local n = 0
for _, q in ipairs(redis.call('SMEMBERS', 'queues')) do
  n = n + redis.call('LLEN', 'queue:' .. q)
end
n = n + redis.call('ZCOUNT', 'schedule', '-inf', now) + redis.call('ZCOUNT', 'retry', '-inf', now)
for _, p in ipairs(redis.call('SMEMBERS', 'processes')) do
  n = n + (tonumber(redis.call('HGET', p, 'busy')) or 0)
end
return n
