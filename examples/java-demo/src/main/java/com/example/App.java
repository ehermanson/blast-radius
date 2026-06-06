package com.example;

import com.example.service.EmailService;

public class App {
    public String register(String email) {
        return EmailService.sendEmail(email);
    }
}
