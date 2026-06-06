require_relative "../models/user"
require_relative "../utils/formatter"

class EmailService
  def self.send_email(email)
    user = User.new(email)
    Formatter.format_subject(user.email)
  end
end
