# Usage: rails runner /seed/snooze.rb <minutes>
# Creates (once) an account with an API inbox, then a new conversation snoozed
# until <minutes> from now, the same way the API's toggle_status does
# (status: snoozed, snoozed_until set). Prints the conversation's display id.
minutes = Integer(ARGV.fetch(0))

account = Account.find_or_create_by!(name: 'ChronoStack test')
inbox = account.inboxes.first || begin
  channel = Channel::Api.create!(account: account)
  account.inboxes.create!(name: 'API', channel: channel)
end

contact = account.contacts.create!(name: "Customer #{SecureRandom.hex(3)}")
contact_inbox = ContactInbox.create!(contact: contact, inbox: inbox, source_id: SecureRandom.uuid)
conversation = Conversation.create!(account: account, inbox: inbox, contact: contact, contact_inbox: contact_inbox)
conversation.update!(status: :snoozed, snoozed_until: minutes.minutes.from_now)

puts conversation.display_id
