require_relative "app/services/email_service"

class App
  def self.register(email)
    EmailService.send_email(email)
  end
end
