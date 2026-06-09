package com.example.report;

import com.example.util.*;

public class ReportService {
    public static String report(String email) {
        return Texts.upper(Formatter.formatSubject(email));
    }
}
