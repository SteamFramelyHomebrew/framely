#pragma once
#include "vendor/json.hpp"
#include <string>

// Native close commands are an optimization; this durable state also repairs
// a missed command without letting an old close affect a newly opened editor.
inline bool framing_editor_should_close(const nlohmann::json& state,const std::string& editor_id){
 if(!state.is_object()||!state.contains("open")||!state["open"].is_boolean()||!state.contains("id")||!state["id"].is_string())return false;
 auto id=state["id"].get<std::string>();
 return state["open"].get<bool>()?id!=editor_id:id==editor_id;
}
