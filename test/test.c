#include<stdio.h>
#include<stdlib.h>

void main()
{
	int i = 42;
	float f = 98;
	double d = 3.14;
	
	while (1) {
		printf("i:%d, f:%f, d:%f\n>", i, f, d);
		
		char *line = 0;
		size_t len = 0;
		ssize_t lineSize =  getline(&line, &len, stdin);
		if (lineSize > 1) {
			switch (*line) {
				case 'f':
					f = strtof(line + 1, NULL);
				break;
				case 'd':
					d = strtod(line + 1, NULL);
				break;
				case 'a':
					f = strtof(line + 1, NULL);
					d = strtod(line + 1, NULL);
					i = strtol(line + 1, NULL, 10);
				break;
				default:
					i = strtol(line, NULL, 10);
				break;
			}
		}
		
		free(line);
	}
	
}
