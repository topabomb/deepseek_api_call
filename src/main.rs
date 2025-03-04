use std::{env};

use base64::{engine::general_purpose, Engine};
use openai::{
    chat::{
        ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole,
    },
    *,
};
use reqwest::{Client, Error as RequestError, header};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Serialize, Deserialize)]
struct Category {
    id: String,
    language: String,
    name: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct CategoryResponse {
    data: Vec<Category>,
    message: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Word {
    word: String,
    exp: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WordResponse {
    data: Vec<Word>,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExampleSentence {
    cn: String,
    en: String,
}
#[derive(Debug,Serialize,Deserialize)]
struct TextToSpeechResponse{
    audioContent:String,
}
async fn get_study_list(language: &str, auth_token: &str) -> Result<Vec<Category>, RequestError> {
    let client = Client::new();

    let url = format!(
        "https://api.frdic.com/api/open/v1/studylist/category?language={}",
        language
    );

    let response = client
        .get(&url)
        .header(header::AUTHORIZATION, auth_token)
        .send()
        .await?
        .json::<CategoryResponse>()
        .await?;
    Ok(response.data)
}
async fn get_words(category: &Category, auth_token: &str) -> Result<Vec<Word>, RequestError> {
    let client = Client::new();

    let url = format!(
        "https://api.frdic.com/api/open/v1/studylist/words/0?language=en&category_id={}",
        category.id
    );
    let resp = client
        .get(&url)
        .header(header::AUTHORIZATION, auth_token)
        .send()
        .await?
        .json::<WordResponse>()
        .await?;

    Ok(resp.data)
}
//FIXME:该函数返回ExampleSentence数组，英文例句中首先按字母复述了单词，再播放完整例句，其实不合理；复述单词跟包含SSML的例句不应该交由AI处理
async fn get_example_sentence(api_key:&str,word: &str) -> Result<Vec<ExampleSentence>, OpenAiError> {
    println!("get_example_sentence pending for {}", word);
    let cred = Credentials::new(
        api_key,
        "https://api.deepseek.com/v1",
    );
    let messages = vec![
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::System,
            content: Some("你是一个精通各个专业的翻译专家。".to_string()),
            name: None,
            function_call: None,
            tool_call_id: None,
            tool_calls: None,
        },
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::User,
            content: Some(format!(r#"
英文单词{}生成3个例句，领域范围为{}；
你的响应包括原文及中文翻译，输出格式使用JSON数组，英文key为en，中文key为cn；
中文例句直接按文本处理，不需要SSML格式；
英文例句严格采用SSML 格式，即以<speak>封装整个例句；例句中，首先使用标记<say-as interpret-as="characters">重复这个单词，然后使用<break time=\"1s\"/>停顿1秒，最后是完整的英文例句，完整例句中无需SSML标记，完整例句中也无需各种除,.之外的符号;
不要回复多余的内容，不要包含Markdown或Html的标记，不要返回```json。"#
            ,word,"RUST编程语言").to_string()),
            name: None,
            function_call: None,
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let chat = ChatCompletion::builder("deepseek-chat", messages)
        .credentials(cred)
        .create()
        .await?;
    let resp = chat.choices.first().unwrap().message.clone();
    let msg = resp.content.unwrap().trim().to_string();
    println!("{msg}");
    let rows: Vec<ExampleSentence> = serde_json::from_str(msg.as_str()).unwrap();
    if rows.len() == 3 {
        Ok(rows)
    } else {
        println!("{:#?} say : {}", resp.role, msg);
        panic!("err.")
    }
}
async fn texttospeech(api_key:&str,ssml:&str,path:&str) -> Result<String, RequestError> {
    let client = Client::new();

    let request_body: Value = json!({
        "input": {"ssml": ssml},
        "voice": {
            "languageCode": "en-US",
            "name": "en-US-Neural2-A"
        },
        "audioConfig": {"audioEncoding": "MP3"}
    });

    let response = client
        .post("https://texttospeech.googleapis.com/v1/text:synthesize")
        .header("X-goog-api-key", api_key)
        .header(header::CONTENT_TYPE, "application/json")
        .json(&request_body)
        .send()
        .await?;

    let resp:TextToSpeechResponse= response.json().await.unwrap();
    let audio_bytes = general_purpose::STANDARD.decode(resp.audioContent).unwrap();
        let mut file = File::create(path).await.unwrap();
        file.write_all(&audio_bytes).await.unwrap();

    Ok(String::from(path))
}
#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let eu_token = env::var("EUDIC_API").expect("dotenv need var:EUDIC_API");
    let ds_token = env::var("DEEPSEEK_API").expect("dotenv need var:DEEPSEEK_API");
    let gg_token=env::var("GOOGLE_API").expect("dotenv need var:GOOGLE_API");


    let categorys = match get_study_list("en", &eu_token).await {
        Ok(v) => v,
        Err(e) => vec![],
    };

    for category in categorys {
        let words = get_words(&category, &eu_token).await.unwrap();
        let list: Vec<String> = words.into_iter().map(|x| x.word).collect();
        println!("category({category:?}),words:{:?}", list);

        for word in list {
            let example_sentence = get_example_sentence(&ds_token,word.as_str()).await;
            println!("{example_sentence:?}");
            for (i,s) in example_sentence.unwrap().iter().enumerate(){
                texttospeech(&gg_token, &s.en,format!("data/{word}-{i}.mp3").as_str()).await.unwrap();
            }
        }
    }
}
