// SPDX-License-Identifier: GPL-3.0-only
// Derived from Play Integrity Fork native config handling (23ec56c).
// Modified 2026-10-24: scalar normalization, rejected invalid UTF-8 and bounded parsing.
#include "transport.hpp"
#include "settings.hpp"
#include <charconv>
#include <sstream>
namespace pif {
int number(const nlohmann::json &value, int fallback) {
    if (!value.is_string()) return fallback;
    const auto text = value.get<std::string>();
    int result = 0;
    auto parsed = std::from_chars(text.data(), text.data() + text.size(), result);
    return parsed.ec == std::errc{} && parsed.ptr == text.data() + text.size() && result >= 0 && result <= 100 ? result : fallback;
}
nlohmann::json parse(const std::string &raw) {
    if (raw.size() > max_config) return {};
    nlohmann::json result;
    if (nlohmann::json::accept(raw, true)) result = nlohmann::json::parse(raw, nullptr, false, true);
    else {
        result = nlohmann::json::object();
        std::istringstream stream(raw);
        std::string line;
        while (std::getline(stream, line)) {
            const auto first = line.find_first_not_of(" \t\r");
            if (first == std::string::npos || line[first] == '#') continue;
            auto delimiter = line.find('=', first);
            if (delimiter == std::string::npos) return {};
            auto key = line.substr(first, delimiter - first);
            auto end = key.find_last_not_of(" \t\r");
            if (end == std::string::npos) return {};
            key.resize(end + 1);
            auto value = line.substr(delimiter + 1);
            auto comment = value.find('#');
            if (comment != std::string::npos) value.resize(comment);
            auto begin = value.find_first_not_of(" \t\r");
            value = begin == std::string::npos ? "" : value.substr(begin);
            end = value.find_last_not_of(" \t\r");
            if (end != std::string::npos) value.resize(end + 1);
            result[key] = value;
        }
    }
    if (!result.is_object() || result.size() > 512) return {};
    for (auto &entry : result.items()) {
        if (entry.key().empty() || entry.key().size() > 128) return {};
        // Match the standalone CLI's accepted JSON scalar representation.
        if (entry.value().is_boolean()) entry.value() = entry.value().get<bool>() ? "1" : "0";
        else if (entry.value().is_number()) entry.value() = entry.value().dump();
        else if (entry.value().is_null()) entry.value() = "";
        else if (!entry.value().is_string()) return {};
        const auto value = entry.value().get<std::string>();
        if (value.size() > 4096 || value.find_first_of(std::string("\0\n\r", 3)) != std::string::npos) return {};
    }
    // Prop input does not pass through the JSON parser. Reject byte sequences that
    // the JSON serializer would replace, before the exception-free injection build
    // serializes Build fields. Valid strings and unknown fields remain unchanged.
    auto checked = nlohmann::json::parse(result.dump(-1, ' ', false,
        nlohmann::json::error_handler_t::replace), nullptr, false);
    return checked == result ? result : nlohmann::json::object();
}
}

namespace pif {
Settings settings(nlohmann::json config, bool vending) {
    Settings result; result.vending=vending;
    auto take=[&](const char *key,int fallback) { auto value=config.contains(key)?number(config[key],fallback):fallback; config.erase(key); return value; };
    result.verboseLogs=take("verboseLogs",0);
    result.spoofBuild=take("spoofBuild",1); result.spoofProps=take("spoofProps",1);
    result.spoofProvider=take("spoofProvider",1); result.spoofSignature=take("spoofSignature",0);
    result.spoofVendingSdk=take("spoofVendingSdk",0);
    auto finger=config.value("spoofVendingFinger",std::string("0")); config.erase("spoofVendingFinger");
    if (finger=="0" || finger=="1") {
        result.spoofVendingFinger=finger=="1";
        result.vendingFingerprint=config.value("FINGERPRINT",std::string());
        if(result.vendingFingerprint.empty()) result.spoofVendingFinger=0;
    } else { result.spoofVendingFinger=1; result.vendingFingerprint=finger; }
    if(vending) {
        result.spoofBuild=result.spoofProps=result.spoofProvider=result.spoofSignature=0;
        result.build=nlohmann::json::object(); return result;
    }
    result.spoofVendingFinger=result.spoofVendingSdk=0;
    result.build=nlohmann::json::object();
    for(const auto &entry:config.items()) {
        if(entry.key().find_first_of("*.")!=std::string::npos) {
            if(!entry.value().get<std::string>().empty()) result.properties.emplace(entry.key(),entry.value().get<std::string>());
        } else result.build[entry.key()]=entry.value();
    }
    return result;
}
}
