# 독립 합성 DNG 센서 예제

`generate_raw_inputs.py`의 자체 TIFF/DNG 인코더가 만든20개 원본이다. 선형RGB·흑백·Bayer 센서 샘플, 16비트 정수/32비트 float·두 byte order·8가지 방향·기본 자르기와 알려진 D65 카메라 행렬/AsShotNeutral을 포함한다. 미리보기 JPEG를 사용하지 않는다. manifest에 원본 SHA-256·센서 값·메타데이터를 기록한다.

`verify_raw_outputs.py`는 파일의 TIFF 태그와 실제 센서 바이트를 독립 파싱하고 NumPy로 검정/흰색 정규화·행렬 역변환·화이트 밸런스·선형 EV·sRGB 곡선·자르기/방향을 계산한다. 40개 촬영값/중립 결과와 사용자 노출 결과·프로젝트의 원본 바이트를 비교한다. Bayer는 일정한 알려진 채널의 내부를 비교하며 PPG의 경계 정책과 실제 카메라 디테일 화질은 검증 완료로 표시하지 않는다.

실제 카메라/압축 변형·X-Trans/4색 CFA 전수·다중 광원/DNG 룩 테이블·ICC/자동 렌즈 프로파일·Photoshop/Compositor 외관 비교는 남은 목표다. RAW 쓰기와 형식 전체 완전 호환을 주장하지 않는다.

형식 기준: [Adobe DNG 명세/SDK](https://helpx.adobe.com/ca/camera-raw/desktop/dng-and-file-formats/digital-negative.html), 디코더 [Rawler 0.8.0 원본](https://github.com/dnglab/dnglab/tree/ae01bcb2d0f8a74f9dfbb9f7b5c7e315c8e668b7/rawler).

화이트 밸런스 스포이드의 자체274조건/30000픽셀은 `verify_raw_white_balance_outputs.py`가 센서 태그/바이트와 NumPy 행렬/정규화/평균을 별도로 계산한다. 8방향/기본 자르기·선형/흑백·Bayer 내부와16/32비트를 포함하며 실제 카메라/4색 CFA/다중 광원 목표는 유지한다.
