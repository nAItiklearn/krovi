from openai import AsyncOpenAI
from typing import Any
import os
from dotenv import load_dotenv

load_dotenv()
class LLMClient:
    def __init__(self)-> None:
        #starts as none when I create the object
        self._client: AsyncOpenAI | None=None
        
    def get_client(self)-> AsyncOpenAI:
        #if ts the 1st time , create the object
        if self._client is None:
            api_key = os.getenv("OPENROUTER_API_KEY")
            if not api_key:
                raise ValueError("OPENROUTER_API_KEY is not set")
             
            self._client=AsyncOpenAI(
                api_key=api_key,
                base_url='https://openrouter.ai/api/v1')
            
        return self._client
    async def close(self) -> None:
        '''using this the client get set back to none and a new object can be created'''
        if self._client:
            await self._client.close()
            self._client=None
    
    async def chat_completion(self, messages:list[dict[str, Any]], stream:bool=True):
        """messages is a paramter everytime the whole messages inc the previous commands get sent to the llm
            , unlike in a chatbot. strem is used to determine how llm delivers the response"""
        client= self.get_client()
        kwargs ={
            "model":"dots-studio/dots-3-note-preview:free",
            "messages": messages,
            "stream": stream,
        }
        if stream:
             await self._stream_response(client, kwargs) ##allows user to see words popping out in real time
        else:
             await self._non_stream_response(client, kwargs) #models writes the whole ans and sends at once 
    async def _stream_response(self, client, kwargs):
        pass
    async def _non_stream_response(self, client: AsyncOpenAI , kwargs: dict[str, Any]):
        response = await client.chat.completions.create(**kwargs)
        print(response.choices[0].message.content)
            