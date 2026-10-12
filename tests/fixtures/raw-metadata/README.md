# 독립 RAW EXIF 예제

자체 TIFF/EXIF IFD 인코더로 기존 합성 센서 바이트를 유지해4종을 생성했다. 별도 EXIF IFD와 루트 렌즈 태그의 우선순위, 두 byte order, 약분하지 않은 분수, 32비트 ISO70000, 분모0/5000바이트 이름을 포함한다. manifest의 원본/센서 SHA를 별도 파서로 확인한다. 실제 카메라/MakerNote 전수 또는 전체 EXIF 출력 검증과 구분한다.
