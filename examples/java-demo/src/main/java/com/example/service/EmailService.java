package com.example.service;

import com.example.model.User;
import com.example.util.Formatter;

public class EmailService {
    public static String sendEmail(String email) {
        User user = new User(email);
        return Formatter.formatSubject(user.email);
    }
}
